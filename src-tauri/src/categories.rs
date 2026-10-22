//! File categories used for colouring the treemap and for aggregate stats.

pub const CAT_OTHER: u8 = 0;
pub const CAT_VIDEO: u8 = 1;
pub const CAT_AUDIO: u8 = 2;
pub const CAT_IMAGE: u8 = 3;
pub const CAT_ARCHIVE: u8 = 4;
pub const CAT_DOCUMENT: u8 = 5;
pub const CAT_CODE: u8 = 6;
pub const CAT_PROGRAM: u8 = 7;
pub const CAT_DISKIMAGE: u8 = 8;
pub const CAT_GAMEDATA: u8 = 9;
pub const CAT_DATABASE: u8 = 10;
pub const CAT_SYSTEM: u8 = 11;
pub const CAT_FOLDER: u8 = 255;

pub const CATEGORY_NAMES: [&str; 12] = [
    "Other",
    "Video",
    "Audio",
    "Images",
    "Archives",
    "Documents",
    "Code & projects",
    "Programs & libraries",
    "Disk images & VMs",
    "Game & engine data",
    "Databases & caches",
    "System & logs",
];

pub fn ext_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => &name[i + 1..],
        _ => "",
    }
}

pub fn category_for_ext(ext: &str) -> u8 {
    let mut buf = [0u8; 16];
    let b = ext.as_bytes();
    if b.is_empty() || b.len() > 15 {
        return CAT_OTHER;
    }
    for (i, c) in b.iter().enumerate() {
        buf[i] = c.to_ascii_lowercase();
    }
    let e = std::str::from_utf8(&buf[..b.len()]).unwrap_or("");
    match e {
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg" | "ts" | "m2ts" | "vob" | "braw" | "mxf" => {
            // .ts is ambiguous (TypeScript); treat tiny ones as code is not knowable here, prefer video only for large files elsewhere
            if e == "ts" { CAT_CODE } else { CAT_VIDEO }
        }
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" | "aiff" | "mid" | "midi" => CAT_AUDIO,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "heif" | "raw" | "cr2" | "cr3" | "nef" | "arw" | "dng" | "psd" | "ai" | "svg" | "ico" | "exr" | "hdr" | "tga" | "dds" => CAT_IMAGE,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "cab" | "tgz" | "lz4" | "lzma" => CAT_ARCHIVE,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "rtf" | "odt" | "ods" | "csv" | "epub" | "mobi" => CAT_DOCUMENT,
        "rs" | "js" | "jsx" | "tsx" | "py" | "c" | "h" | "cpp" | "hpp" | "cs" | "java" | "go" | "rb" | "php" | "html" | "css" | "json" | "toml" | "yaml" | "yml" | "xml" | "sh" | "ps1" | "bat" | "cmd" | "lua" | "kt" | "swift" | "rlib" | "rmeta" | "o" | "obj" | "pdb" | "map" | "lock" | "pyc" | "class" | "jar" | "whl" | "node" | "d" => CAT_CODE,
        "exe" | "dll" | "msi" | "msix" | "appx" | "msp" | "lib" | "ocx" | "cpl" | "scr" | "com" | "so" | "dylib" | "a" => CAT_PROGRAM,
        "iso" | "img" | "vhd" | "vhdx" | "vmdk" | "vdi" | "wim" | "esd" | "qcow2" | "ova" | "ovf" | "bin" | "gguf" | "safetensors" | "ckpt" | "pt" | "pth" | "onnx" => CAT_DISKIMAGE,
        "pak" | "bsa" | "vpk" | "wad" | "uasset" | "umap" | "upk" | "unity3d" | "assets" | "bundle" | "forge" | "big" | "arc" | "ba2" | "rpf" | "bank" | "fsb" | "ucas" | "utoc" | "sb" | "mpq" | "sav" | "pck" | "gcf" | "ndu" | "xnb" | "rbf" | "resource" | "ress" | "fbx" | "blend" | "obj3d" | "usd" | "usdz" | "gltf" | "glb" | "vmt" | "vtf" | "mdl" => CAT_GAMEDATA,
        "db" | "sqlite" | "sqlite3" | "mdf" | "ldf" | "ndf" | "bak" | "idx" | "ldb" | "mdb" | "accdb" | "parquet" | "arrow" | "dat" | "cache" | "tmp" | "temp" | "swp" | "etl" | "ost" | "pst" | "nupkg" | "wal" | "shm" => CAT_DATABASE,
        "sys" | "log" | "dmp" | "mdmp" | "hdmp" | "evtx" | "cat" | "mum" | "manifest" | "mui" | "nls" | "cdf-ms" | "etw" | "regtrans-ms" | "blf" => CAT_SYSTEM,
        _ => CAT_OTHER,
    }
}

pub fn category_for_name(name: &str) -> u8 {
    if name.len() == 11 || name.len() == 12 {
        if name.eq_ignore_ascii_case("pagefile.sys") || name.eq_ignore_ascii_case("hiberfil.sys") || name.eq_ignore_ascii_case("swapfile.sys") {
            return CAT_SYSTEM;
        }
    }
    category_for_ext(ext_of(name))
}
