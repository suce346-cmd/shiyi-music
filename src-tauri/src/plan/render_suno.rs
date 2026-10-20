//! SongPlan → Suno 四栏（BLUEPRINT_v2 §2 / P1-4）。纯函数，唯一渲染点。
//!
//! - Style：全局六层按序逗号拼接（genre → bpm/key/meter → vocal → instruments → arc → mix），恒英文（§3.7 另批）。
//! - Lyrics：每段 `[Label | cue, cue]` 头 + 歌词行；cue = 段内正向 cue + 在场乐器 + `energy N`
//!   + 段内负向 cue 的正向改写。段头语法由 P2 探针定，[`SECTION_HEADER`] 单点切换。
//! - Exclude：`global.negative`（Suno 无段落级 Exclude；段内负向只做正向改写，不并入全局，
//!   避免"某段不要鼓"误伤全曲）。
//! - Settings：Weirdness / Style Influence / Variety（缺省 0）/ 可选 Audio Influence 与人声性别。

use super::{SectionKind, SongPlan, VocalGender};

/// 段头语法（P2 探针结论回填前的缺省：竖线内联 cue）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionHeader {
    /// `[Verse 1 | cue, cue]`
    PipeInline,
    /// `[Verse 1]` + 下一行 `(cue, cue)`
    BracketThenParen,
}

pub const SECTION_HEADER: SectionHeader = SectionHeader::PipeInline;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub title: Option<String>,
    pub style: String,
    pub lyrics: String,
    pub exclude: String,
    pub settings: String,
}

fn push_nonempty(out: &mut Vec<String>, v: &str) {
    let t = v.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
}

/// Style 栏标签序列（validate 的标签数门与渲染同源）
pub fn style_tags(p: &SongPlan) -> Vec<String> {
    let g = &p.global;
    let mut t = Vec::new();
    g.genre.iter().for_each(|x| push_nonempty(&mut t, x));
    if let Some(b) = g.bpm {
        t.push(format!("{b} BPM"));
    }
    if let Some(k) = &g.key {
        push_nonempty(&mut t, k);
    }
    if let Some(m) = &g.meter {
        push_nonempty(&mut t, m);
    }
    g.vocal.iter().for_each(|x| push_nonempty(&mut t, x));
    g.instruments.iter().for_each(|x| push_nonempty(&mut t, x));
    if let Some(a) = &g.arc {
        push_nonempty(&mut t, a);
    }
    g.mix.iter().for_each(|x| push_nonempty(&mut t, x));
    t
}

/// 段内负向 cue 的正向改写：`no drums` / `without drums` → `drums drop out`；已是正向则原样。
pub fn rewrite_negative_local(cue: &str) -> String {
    let t = cue.trim();
    let l = t.to_lowercase();
    for p in ["no ", "without "] {
        if l.starts_with(p) {
            return format!("{} drop out", t[p.len()..].trim());
        }
    }
    t.to_string()
}

fn kind_is_instrumental_only(k: SectionKind) -> bool {
    matches!(k, SectionKind::Intro | SectionKind::Interlude | SectionKind::Outro | SectionKind::End | SectionKind::Drop | SectionKind::Breakdown)
}

pub fn section_cues(s: &super::Section) -> Vec<String> {
    let mut c = Vec::new();
    s.positive_local.iter().for_each(|x| push_nonempty(&mut c, x));
    s.instruments.iter().for_each(|x| push_nonempty(&mut c, x));
    if let Some(e) = s.energy {
        c.push(format!("energy {e}"));
    }
    for n in &s.negative_local {
        let r = rewrite_negative_local(n);
        push_nonempty(&mut c, &r);
    }
    if s.lines.is_empty() && kind_is_instrumental_only(s.kind) {
        c.push("instrumental".into());
    }
    c
}

pub fn render(p: &SongPlan) -> Rendered {
    let style = style_tags(p).join(", ");
    let mut lyrics = String::new();
    for (i, s) in p.sections.iter().enumerate() {
        if i > 0 {
            lyrics.push('\n');
        }
        let cues = section_cues(s).join(", ");
        match SECTION_HEADER {
            SectionHeader::PipeInline if !cues.is_empty() => lyrics.push_str(&format!("[{} | {}]\n", s.label.trim(), cues)),
            SectionHeader::BracketThenParen if !cues.is_empty() => {
                lyrics.push_str(&format!("[{}]\n({})\n", s.label.trim(), cues))
            }
            _ => lyrics.push_str(&format!("[{}]\n", s.label.trim())),
        }
        for l in &s.lines {
            let t = l.trim();
            if !t.is_empty() {
                lyrics.push_str(t);
                lyrics.push('\n');
            }
        }
    }
    let exclude = p.global.negative.iter().map(|x| x.trim()).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
    let st = &p.settings;
    let mut settings = format!(
        "Weirdness={} | Style Influence={} | Variety={}",
        st.weirdness, st.style_influence, st.variety
    );
    if let Some(a) = st.audio_influence {
        settings.push_str(&format!(" | Audio Influence={a}"));
    }
    if let Some(g) = st.vocal_gender {
        settings.push_str(match g {
            VocalGender::Male => " | Vocal Gender=Male",
            VocalGender::Female => " | Vocal Gender=Female",
        });
    }
    Rendered { title: p.title.clone(), style, lyrics: lyrics.trim_end().to_string(), exclude, settings }
}

/// 交付包文本（前端结果区当前按整段展示；P1-11 改分块复制后仍以本函数为唯一排版点）。
/// 栏名行是前端/历史的稳定锚点（`Style:` / `Exclude:` / `Settings:` / `Lyrics:`）。
pub fn to_package_text(r: &Rendered) -> String {
    let mut out = String::new();
    if let Some(t) = r.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        out.push_str(&format!("Title: {t}\n\n"));
    }
    out.push_str(&format!("Style: {}\n\n", r.style));
    if !r.exclude.is_empty() {
        out.push_str(&format!("Exclude: {}\n\n", r.exclude));
    }
    out.push_str(&format!("Settings: {}\n\nLyrics:\n{}\n", r.settings, r.lyrics));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::fixtures::minimal_plan;

    /// 固定 SongPlan → 渲染快照逐字节一致（BLUEPRINT P1-4 验证项）
    #[test]
    fn render_snapshot_byte_identical() {
        let r = render(&minimal_plan());
        let text = to_package_text(&r);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/render/minimal_plan.txt");
        if std::env::var("SHIYI_UPDATE_SNAPSHOTS").is_ok() {
            std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
            std::fs::write(path, &text).unwrap();
        }
        let want = std::fs::read_to_string(path).expect("快照缺失：SHIYI_UPDATE_SNAPSHOTS=1 生成");
        assert_eq!(text, want);
    }

    #[test]
    fn negative_local_rewritten_not_merged_into_exclude() {
        let r = render(&minimal_plan());
        assert!(r.lyrics.contains("drums drop out"), "{}", r.lyrics);
        assert!(!r.lyrics.to_lowercase().contains("no drums"));
        assert_eq!(r.exclude, "edm drops", "段内负向不得并入全局 Exclude");
    }

    #[test]
    fn style_order_and_settings_line() {
        let r = render(&minimal_plan());
        assert!(r.style.starts_with("chinese folk ballad, 72 BPM, 4/4, vocal_breathy_close"), "{}", r.style);
        assert_eq!(r.settings, "Weirdness=30 | Style Influence=70 | Variety=0 | Vocal Gender=Female");
    }

    #[test]
    fn instrumental_section_without_lines_is_marked() {
        let r = render(&minimal_plan());
        assert!(r.lyrics.starts_with("[Intro | rain ambience, guzheng, energy 1, instrumental]"), "{}", r.lyrics);
    }
}
