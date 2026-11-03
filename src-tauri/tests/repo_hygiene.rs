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
    assert_eq!(
        pkg["version"].as_str(),
        Some(cargo_ver),
        "package.json 与 Cargo.toml 版本不一致"
    );
    assert!(
        changelog.contains("\n## Unreleased"),
        "CHANGELOG 缺 Unreleased 段"
    );
    assert!(
        changelog.contains(&format!("\n## {cargo_ver}（"))
            || changelog.contains(&format!("\n## {cargo_ver} ")),
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

/// P0-10：前端写文件权限收窄到 $APPDATA/$DOWNLOAD/$DOCUMENT；保存对话框所选路径由
/// tauri-plugin-dialog 运行期加入 fs scope（导出到任意目录仍可用），不再需要全盘 `**`
#[test]
fn fs_write_scope_is_narrowed() {
    let cap =
        std::fs::read_to_string(repo().join("src-tauri/capabilities/default.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&cap).unwrap();
    let perms = v["permissions"].as_array().unwrap();
    for p in perms {
        if let Some(allow) = p.get("allow").and_then(|a| a.as_array()) {
            for a in allow {
                let path = a["path"].as_str().unwrap_or("");
                assert!(
                    ["$APPDATA/", "$DOWNLOAD/", "$DOCUMENT/"]
                        .iter()
                        .any(|pre| path.starts_with(pre)),
                    "fs scope 超出允许目录: {path}"
                );
            }
        }
    }
}

/// P0-10：日志不落模型输出正文（信封门旧日志记录方案前 300 字）
#[test]
fn logs_do_not_record_model_output_head() {
    let src =
        std::fs::read_to_string(repo().join("src-tauri/src/commands/orchestrator.rs")).unwrap();
    let needle = ["output", "_head"].concat();
    assert_eq!(src.matches(needle.as_str()).count(), 0);
}

/// P1-J（§5.1 #13）：禁止无主 `#[allow(dead_code)]`——测试专用用 `#[cfg(test)]`，无调用者直接删
#[test]
fn no_blanket_dead_code_allows() {
    let mut hits = Vec::new();
    let needle = ["#[allow(", "dead_code)]"].concat();
    fn walk(dir: &Path, needle: &str, hits: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, needle, hits);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                let s = std::fs::read_to_string(&p).unwrap();
                if s.contains(needle) {
                    hits.push(p.display().to_string());
                }
            }
        }
    }
    walk(&repo().join("src-tauri/src"), &needle, &mut hits);
    assert!(hits.is_empty(), "存在 allow(dead_code): {hits:?}");
}

/// P1-J（§8 / D10 / D14）：已删除资产不得回流
#[test]
fn removed_junk_assets_stay_removed() {
    let r = repo();
    for p in [
        "public/tauri.svg",
        "public/vite.svg",
        "src-tauri/icons/android",
        "src-tauri/icons/ios",
        "src-tauri/icons/StoreLogo.png",
        "src-tauri/icons/Square30x30Logo.png",
        "src/utils/speechOrder.test.ts",
        "docs/2026-09-14-v055-keychain-fix-plan.md",
        "docs/qa/results_v3_final.jsonl",
        "docs/qa/results_v4_final.jsonl",
        "docs/qa/results_12round_final.jsonl",
        "docs/qa/b02-root-cause.md",
        "docs/qa/p1-3-4-acceptance.md",
        "docs/qa/p1-6-scenario-reclass.md",
        "docs/qa/p1-13-ambiguities.md",
        "docs/audit-2026-10-19/REPORT.md",
        "docs/audit-2026-10-19/README.md",
        "docs/audit-2026-10-19/p3-reaudit.md",
        "docs/audit-2026-10-19/p3-retire-text-validators.md",
        "docs/audit-2026-10-19/BLUEPRINT_v2.md",
        "docs/audit-2026-10-19/full-review-2026-10-20.md",
    ] {
        assert!(!r.join(p).exists(), "已清理资产回流: {p}");
    }
    // 上移后的真源必须在
    for p in [
        "docs/BLUEPRINT_v2.md",
        "docs/BLUEPRINT_v3_CONTRACT.md",
        "docs/full-review-2026-10-20.md",
        "docs/RELEASE.md",
    ] {
        assert!(r.join(p).is_file(), "缺少文档真源: {p}");
    }
    let gi = std::fs::read_to_string(r.join(".gitignore")).unwrap();
    for pat in [
        "graphify-out/",
        ".workbuddy/",
        ".DS_Store",
        ".env",
        ".env.*",
        ".secrets/",
        "*.pem",
        "*.key",
    ] {
        assert!(gi.lines().any(|l| l.trim() == pat), ".gitignore 缺 {pat}");
    }
}

/// v3 契约守护：全仓源码、知识库与文档严禁出现外部参考项目的身份指纹或连接指向
#[test]
fn no_external_reference_fingerprints() {
    let r = repo();
    let needles: Vec<String> = [
        ["lyric-writing", "-skills"],
        ["music-composition", "-skills"],
        ["LYR-", "SPEC"],
        ["ARR-", "SPEC"],
        ["LYR-", "LINT"],
        ["SPEC-", "LINT"],
        ["lw-", "00"],
        ["mc-", "00"],
    ]
    .iter()
    .map(|p| p.concat())
    .collect();

    fn walk(dir: &Path, needles: &[String], hits: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, needles, hits);
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|ext| {
                ["rs", "ts", "tsx", "csv", "md", "json", "txt"].contains(&ext)
            }) {
                if p.file_name().and_then(|n| n.to_str()) == Some("repo_hygiene.rs") {
                    continue;
                }
                let s = std::fs::read_to_string(&p).unwrap_or_default();
                for n in needles {
                    if s.contains(n.as_str()) {
                        hits.push(format!("{}: {}", p.display(), n));
                    }
                }
            }
        }
    }

    let mut hits = Vec::new();
    for sub in [
        "src",
        "src-tauri/src",
        "src-tauri/knowledge",
        "src-tauri/tests",
        "docs",
    ] {
        walk(&r.join(sub), &needles, &mut hits);
    }
    assert!(
        hits.is_empty(),
        "发现外部参考项目身份指纹残留：\n{}",
        hits.join("\n")
    );
}
