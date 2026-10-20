//! 全量复审 2026-10-20：IPC 命令集成测试（离线、无网络、不碰钥匙串/系统文件管理器）。
//! 覆盖不依赖具体 Wry 窗口的命令；history_* / pipeline_generate / cancel_pipeline 绑定 Wry AppHandle，
//! 其数据层（encode/decode、validate_request）在此直测，窗口层留 GUI 实点。
use suno_prompt_generator_lib::commands::{history, llm, orchestrator};
use suno_prompt_generator_lib::errors::ErrorKind;
use suno_prompt_generator_lib::plan::{douyin, suno_profile};

#[tokio::test]
async fn get_pipeline_meta_lists_all_modes_and_roles() {
    let m = orchestrator::get_pipeline_meta().await.unwrap();
    for k in ["mode_a", "mode_b", "mode_c", "mode_d"] {
        assert!(!m.modes[k].is_empty(), "{k} 应有角色");
        for r in &m.modes[k] {
            assert!(m.roles.contains_key(r), "{k} 角色 {r:?} 缺元数据");
        }
    }
}

#[tokio::test]
async fn round_gate_decide_and_interject_reject_bad_input() {
    assert_eq!(orchestrator::round_gate_decide(None, "continue".into()).await.unwrap_err().kind, ErrorKind::Validation);
    assert_eq!(orchestrator::round_gate_decide(Some("r".into()), "bogus".into()).await.unwrap_err().kind, ErrorKind::Validation);
    assert_eq!(orchestrator::interject_feedback(None, "x".into()).await.unwrap_err().kind, ErrorKind::Validation);
    assert_eq!(orchestrator::interject_feedback(Some("r".into()), "".into()).await.unwrap_err().kind, ErrorKind::Validation);
}

#[test]
fn douyin_meta_and_scene_suggestion() {
    let m = douyin::get_douyin_meta();
    let ids: Vec<_> = m.scenes.iter().map(|s| s.id.as_str()).collect();
    for want in ["emotion", "daily", "dance", "story"] {
        assert!(ids.contains(&want), "{ids:?}");
    }
    assert_eq!(m.default_scene, "daily");
    let s = douyin::suggest_douyin_scenes("跳舞卡点 节奏炸".into());
    assert_eq!(s.first().map(|x| x.scene.as_str()), Some("dance"), "{:?}", s.iter().map(|x| &x.scene).collect::<Vec<_>>());
}

#[test]
fn suno_meta_non_empty() {
    assert!(!suno_profile::get_suno_meta().limits.is_empty());
}

#[tokio::test]
async fn test_api_and_probe_reject_bad_url_before_network() {
    for u in ["ftp://x", "http://127.0.0.1:1", "http://198.18.0.1"] {
        let e = llm::test_api(u.into(), "k".into(), "m".into()).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation, "{u}: {}", e.message);
        let e = llm::probe_structured_caps(u.into(), "k".into(), "m".into()).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation, "{u}: {}", e.message);
    }
}

#[tokio::test]
async fn pipeline_resume_without_checkpoint_errors_cleanly() {
    let app = tauri::test::mock_app();
    let req: suno_prompt_generator_lib::models::PipelineRequest = serde_json::from_value(serde_json::json!({
        "mode": "mode_b", "user_input": "雨伞", "model": "m", "api_key": "k",
        "base_url": "https://api.example.com/v1", "thinking": false
    }))
    .unwrap();
    let e = orchestrator::pipeline_resume(app.handle().clone(), req, "no-such-run-ipc-test".into()).await.unwrap_err();
    assert_ne!(e.kind, ErrorKind::Network, "无检查点应在发请求前报错: {}", e.message);
}

#[test]
fn history_roundtrip_keeps_song_plan_and_rendered() {
    let v = serde_json::json!([{ "id": "1", "mode": "mode_b", "input": "i", "output": "o", "timestamp": 1,
        "song_plan": {"sections": []},
        "rendered": {"title": null, "style": "s", "lyrics": "l", "exclude": "e", "settings": "x"} }]);
    let entries: Vec<history::HistoryEntry> = serde_json::from_value(v).unwrap();
    let back = history::decode_history_file(&history::encode_history_file(&entries)).unwrap();
    let j = serde_json::to_value(&back).unwrap();
    assert_eq!(j[0]["song_plan"]["sections"], serde_json::json!([]));
    assert_eq!(j[0]["rendered"]["style"], "s");
}
