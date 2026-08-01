// 照片墙 Tauri 后端:扫描照片文件夹、多线程生成缩略图缓存、清理缓存
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rayon::prelude::*;
use std::{
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::UNIX_EPOCH,
};
use tauri::{AppHandle, Emitter, Manager};

/// 缩略图最长边:格子最大不过 1/4 屏,1280 结实够用
const THUMB_MAX: u32 = 1280;
const JPEG_QUALITY: u8 = 82;

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "jfif")
    )
}

/// 缓存键 = 原图路径 + 修改时间 + 文件大小的哈希:
/// 二次导入秒开,原图改动自动重转,不重复干活
fn cache_key(path: &Path, mtime: u64, len: u64) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut h);
    mtime.hash(&mut h);
    len.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn exif_orientation(path: &Path) -> u32 {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return 1,
    };
    let mut br = std::io::BufReader::new(file);
    match exif::Reader::new().read_from_container(&mut br) {
        Ok(r) => r
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            .unwrap_or(1),
        Err(_) => 1,
    }
}

/// 按 EXIF 方向摆正,竖拍照片不会躺着
fn apply_orientation(img: image::DynamicImage, o: u32) -> image::DynamicImage {
    match o {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

fn make_thumb(src: &Path, dst: &Path) -> Result<(), String> {
    let img = image::open(src).map_err(|e| e.to_string())?;
    let img = apply_orientation(img, exif_orientation(src));
    let img = if img.width().max(img.height()) > THUMB_MAX {
        img.resize(THUMB_MAX, THUMB_MAX, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    // 先写临时文件再改名,避免中途崩溃留下半截缓存
    let tmp = dst.with_extension("tmp");
    {
        let file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut out = std::io::BufWriter::new(file);
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
        enc.encode_image(&rgb).map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, dst).map_err(|e| e.to_string())
}

#[derive(serde::Serialize, Clone)]
struct Progress {
    done: usize,
    total: usize,
}

fn prepare_folder_inner(app: &AppHandle, dir: &str) -> Result<Vec<String>, String> {
    let root = PathBuf::from(dir);
    if !root.exists() {
        return Err("路径不存在".into());
    }

    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("thumbs");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;

    // 文件夹或单张图片都接受(拖拽可能拖进来一张图)
    let mut files: Vec<PathBuf> = if root.is_file() {
        if is_image(&root) {
            vec![root.clone()]
        } else {
            vec![]
        }
    } else {
        walkdir::WalkDir::new(&root)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| is_image(p))
            .collect()
    };
    files.sort();
    let total = files.len();
    if total == 0 {
        return Err("文件夹里没有找到照片".into());
    }

    let done = AtomicUsize::new(0);
    let thumbs: Vec<Option<String>> = files
        .par_iter()
        .map(|src| {
            let meta = fs::metadata(src).ok();
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let len = meta.map(|m| m.len()).unwrap_or(0);
            let dst = cache.join(format!("{}.jpg", cache_key(src, mtime, len)));
            let ok = dst.exists() || make_thumb(src, &dst).is_ok();
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            if d % 5 == 0 || d == total {
                let _ = app.emit("thumb-progress", Progress { done: d, total });
            }
            if ok {
                let s = dst.to_string_lossy().to_string();
                let _ = app.emit("thumb-ready", s.clone());
                Some(s)
            } else {
                None
            }
        })
        .collect();

    Ok(thumbs.into_iter().flatten().collect())
}

/// 扫描文件夹并生成缩略图;边生成边通过事件把就绪的缩略图推给前端
#[tauri::command]
async fn prepare_folder(app: AppHandle, dir: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || prepare_folder_inner(&app, &dir))
        .await
        .map_err(|e| e.to_string())?
}

/// 清空缩略图缓存,返回释放的 MB 数
#[tauri::command]
fn clear_thumb_cache(app: AppHandle) -> Result<u64, String> {
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("thumbs");
    let mut bytes: u64 = 0;
    if cache.exists() {
        for e in walkdir::WalkDir::new(&cache)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if e.file_type().is_file() {
                bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        fs::remove_dir_all(&cache).map_err(|e| e.to_string())?;
    }
    Ok(bytes / 1024 / 1024)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![prepare_folder, clear_thumb_cache])
        .run(tauri::generate_context!())
        .expect("error while running photo wall");
}
