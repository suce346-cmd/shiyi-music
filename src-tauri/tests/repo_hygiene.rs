//! 仓库卫生守护（P0-8，C-13/C-14）：离线、零网络。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// C-13：当前版本（Cargo.toml / package.json）必须在 CHANGELOG 有版本段，且存在 Unreleased 段
#[test]
fn changelog_covers_current_version() {
    let changelog = std::fs::read_to_string(repo().join("CHANGELOG.md")).unwrap();
    let cargo_ver = env!("CARGO_PKG_VERSION");
    let pkg = std::fs::read_to_string(repo().join("package.json")).unwrap();
    let pkg: serde_json::Value = serde_json::from_str(&pkg).unwrap();
    assert_eq!(pkg["version"].as_str(), Some(cargo_ver), "package.json 与 Cargo.toml 版本不一致");
    assert!(changelog.contains("\n## Unreleased"), "CHANGELOG 缺 Unreleased 段");
    assert!(
        changelog.contains(&format!("\n## {cargo_ver}（")) || changelog.contains(&format!("\n## {cargo_ver} ")),
        "CHANGELOG 缺当前版本 {cargo_ver} 段"
    );
}

/// C-14：docs/qa 下 jsonl 数据不得内容重复（同一份数据两个名字会让统计口径翻倍）
#[test]
fn qa_jsonl_no_duplicate_content() {
    let dir = repo().join("docs").join("qa");
    let mut seen: HashMap<Vec<u8>, String> = HashMap::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        let bytes = std::fs::read(&p).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if let Some(prev) = seen.insert(bytes, name.clone()) {
            panic!("docs/qa 数据重复：{prev} 与 {name} 内容相同");
        }
    }
}
