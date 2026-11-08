//! SongPlan → Suno 四栏（v3 诚实编译器）。纯函数，唯一渲染点。
//!
//! - Style：按 Suno v5/v6 权重梯度从高到低逗号拼接（mood → genre → bpm/key/meter → vocal → instruments → arrangement_hook → arc → mix），恒英文（§3.7 `validate_style_english`）。
//! - Lyrics：每段 `[Label | cue, cue]` 头 + 歌词行；cue = 段内正向局部变化量 (`positive_local`) + 未在 `positive_local` 重复提及的在场乐器 + 减法事件 (`subtraction`) + `energy N`（供前端 `parseEnergy` 提取能量条并在展示/复制时自动剥离） + 段内负向 cue 的正向改写。
//! - Exclude：`global.negative`（Suno 无段落级 Exclude；段内负向只做正向改写，不并入全局，避免"某段不要鼓"误伤全曲）。
//! - Settings：Weirdness / Style Influence / Variety（缺省 0）/ 可选 Audio Influence 与人声性别。

use super::{SectionKind, SongPlan, VocalGender};

/// 段头语法（单一方括号竖线内联 cue：`[Verse 1 | cue, cue]`；
/// 禁用独立行圆括号 `(cue)`——Suno v6 会将独立圆括号行当成背景歌词唱出）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionHeader {
    /// `[Verse 1 | cue, cue]`
    PipeInline,
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

fn push_unique_ci(out: &mut Vec<String>, v: &str) {
    let t = v.trim();
    if t.is_empty() {
        return;
    }
    let tl = t.to_lowercase();
    if !out.iter().any(|existing| existing.to_lowercase() == tl) {
        out.push(t.to_string());
    }
}

/// Style 栏标签序列（按 Suno 权重置顶 mood，validate 的标签数门与渲染同源）
pub fn style_tags(p: &SongPlan) -> Vec<String> {
    let g = &p.global;
    let raw_count = usize::from(g.mood.as_ref().is_some_and(|s| !s.trim().is_empty()))
        + g.genre.len()
        + usize::from(g.bpm.is_some())
        + usize::from(g.key.as_ref().is_some_and(|s| !s.trim().is_empty()))
        + usize::from(g.meter.as_ref().is_some_and(|s| !s.trim().is_empty()))
        + g.vocal.len()
        + g.instruments.len()
        + usize::from(g.arrangement_hook.as_ref().is_some_and(|s| !s.trim().is_empty()))
        + usize::from(g.arc.as_ref().is_some_and(|s| !s.trim().is_empty()))
        + g.mix.len();
    // 当模型在数组字段塞入过多条目（导致总标签 > 15 或展开后变成 800 字冗余大杂烩）时，
    // 编译器自动做高权重收敛裁剪（保持 8–14 个标签、~180–260 字符黄金区间）；
    // 对精简方案（raw_count <= 12，如 minimal_plan 测试夹具）原样输出，确保快照逐字节不变。
    let compact = raw_count > 12;
    let mut t = Vec::new();
    if let Some(mood) = &g.mood {
        push_nonempty(&mut t, mood);
    }
    let genre_cap = if compact { 2 } else { g.genre.len() };
    g.genre.iter().take(genre_cap).for_each(|x| push_nonempty(&mut t, x));
    if let Some(b) = g.bpm {
        t.push(format!("{b} BPM"));
    }
    if let Some(k) = &g.key {
        push_nonempty(&mut t, k);
    }
    if let Some(m) = &g.meter {
        push_nonempty(&mut t, m);
    }
    let vocal_cap = if compact { 1 } else { g.vocal.len() };
    g.vocal
        .iter()
        .take(vocal_cap)
        .for_each(|x| push_nonempty(&mut t, &vocal_mix_expand(x)));
    let inst_cap = if compact {
        crate::rules::INSTRUMENT_MAX
    } else {
        g.instruments.len()
    };
    g.instruments.iter().take(inst_cap).for_each(|x| push_nonempty(&mut t, x));
    if let Some(hook) = &g.arrangement_hook {
        push_nonempty(&mut t, hook);
    }
    if let Some(a) = &g.arc {
        push_nonempty(&mut t, a);
    }
    let mix_cap = if compact {
        if p.mode_meta.douyin.is_some() { 3 } else { 2 }
    } else {
        g.mix.len()
    };
    g.mix
        .iter()
        .take(mix_cap)
        .for_each(|x| push_nonempty(&mut t, &vocal_mix_expand(x)));
    t
}

/// vocal/mix 字段支持 vocal_mix 词表 id（如 V-02 / M-01，渲染展开为 label_en），
/// 也支持直接书写具象英文人声/混音短语（原样透传）。
fn vocal_mix_expand(id: &str) -> String {
    crate::knowledge::vocal_mix_label_en(crate::knowledge::shared_knowledge(), id)
        .unwrap_or_else(|| id.to_string())
}

/// 段内负向 cue 的正向改写：`no drums` / `without drums` → `drums drop out`；已是正向则原样。
pub fn rewrite_negative_local(cue: &str) -> String {
    let t = cue.trim();
    if t.is_empty() {
        return String::new();
    }
    let l = t.to_lowercase();
    for p in ["no ", "without "] {
        if l.starts_with(p) {
            return format!("{} drop out", t[p.len()..].trim());
        }
    }
    if l.contains("drop")
        || l.contains("absent")
        || l.contains("out")
        || l.contains("mute")
        || l.contains("silent")
        || l.contains("strip")
        || l.contains("held off")
    {
        return t.to_string();
    }
    format!("{t} drop out")
}

fn kind_is_instrumental_only(k: SectionKind) -> bool {
    matches!(
        k,
        SectionKind::Intro
            | SectionKind::Interlude
            | SectionKind::Outro
            | SectionKind::End
            | SectionKind::Drop
            | SectionKind::Breakdown
    )
}

pub fn section_cues(s: &super::Section) -> Vec<String> {
    let mut c = Vec::new();
    s.positive_local.iter().for_each(|x| push_nonempty(&mut c, x));
    // 避免 positive_local 已写带奏法的该乐器（如 "piano rolled chords"）时再重复塞入同乐器（如 "felt piano"），
    // 并预留 subtraction / negative_local / energy N 预算，使未在 positive_local 提及的在场乐器始终按标准逻辑补入且不超 DESC_LINE_MAX_CHARS。
    let pos_joined = c.join(" ").to_lowercase();
    let label_len = s.label.trim().chars().count();
    let budget = crate::rules::DESC_LINE_MAX_CHARS.saturating_sub(label_len + 18);
    let sub_reserve: usize = if let Some(sub) = &s.subtraction {
        let r = rewrite_negative_local(sub);
        if r.is_empty() { 0 } else { r.chars().count() + 2 }
    } else {
        s.negative_local
            .iter()
            .map(|n| {
                let r = rewrite_negative_local(n);
                if r.is_empty() { 0 } else { r.chars().count() + 2 }
            })
            .sum()
    };
    let inst_budget = budget.saturating_sub(sub_reserve);
    for inst in &s.instruments {
        let t = inst.trim();
        if t.is_empty() {
            continue;
        }
        let tl = t.to_lowercase();
        let tail_noun = tl.split_whitespace().last().unwrap_or("");
        let already_mentioned = pos_joined.contains(&tl)
            || (tail_noun.len() >= 4
                && !matches!(tail_noun, "kit" | "set" | "pad" | "lead" | "line" | "part")
                && pos_joined.contains(tail_noun));
        if !already_mentioned {
            let cur_len: usize = c.iter().map(|x| x.chars().count() + 2).sum();
            if c.is_empty() || cur_len + t.chars().count() <= inst_budget {
                push_unique_ci(&mut c, t);
            }
        }
    }
    if let Some(sub) = &s.subtraction {
        let r = rewrite_negative_local(sub);
        let cur_len: usize = c.iter().map(|x| x.chars().count() + 2).sum();
        if cur_len + r.chars().count() <= budget {
            push_unique_ci(&mut c, &r);
        }
    }
    if let Some(e) = s.energy {
        c.push(format!("energy {e}"));
    }
    let has_sub = s.subtraction.as_ref().is_some_and(|sub| !sub.trim().is_empty());
    if !has_sub {
        let existing_lower = c.join(" ").to_lowercase();
        for n in &s.negative_local {
            let raw_n = n.trim().to_lowercase();
            let noun = raw_n.trim_start_matches("no ").trim_start_matches("without ").trim();
            if !noun.is_empty() && existing_lower.contains(noun) {
                continue;
            }
            let r = rewrite_negative_local(n);
            let cur_len: usize = c.iter().map(|x| x.chars().count() + 2).sum();
            if cur_len + r.chars().count() <= budget {
                push_unique_ci(&mut c, &r);
            }
        }
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
            SectionHeader::PipeInline if !cues.is_empty() => {
                lyrics.push_str(&format!("[{} | {}]\n", s.label.trim(), cues))
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
    let exclude = p
        .global
        .negative
        .iter()
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
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
    Rendered {
        title: p.title.clone(),
        style,
        lyrics: lyrics.trim_end().to_string(),
        exclude,
        settings,
    }
}

/// 交付包文本（唯一排版点）。
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
    out.push_str(&format!(
        "Settings: {}\n\nLyrics:\n{}\n",
        r.settings, r.lyrics
    ));
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
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/render/minimal_plan.txt"
        );
        if std::env::var("SHIYI_UPDATE_SNAPSHOTS").is_ok() {
            std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
            std::fs::write(path, &text).unwrap();
        }
        let want =
            std::fs::read_to_string(path).expect("快照缺失：SHIYI_UPDATE_SNAPSHOTS=1 生成");
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
        assert!(
            r.style
                .starts_with("chinese folk ballad, 72 BPM, 4/4, breathy vocal, smooth runs"),
            "{}",
            r.style
        );
        assert_eq!(
            r.settings,
            "Weirdness=30 | Style Influence=70 | Variety=0 | Vocal Gender=Female"
        );
    }

    #[test]
    fn instrumental_section_without_lines_is_marked() {
        let r = render(&minimal_plan());
        assert!(
            r.lyrics
                .starts_with("[Intro | rain ambience, guzheng, energy 1, instrumental]"),
            "{}",
            r.lyrics
        );
    }

    #[test]
    fn mood_and_arrangement_hook_and_subtraction_render_cleanly() {
        let mut p = minimal_plan();
        p.global.mood = Some("intimate melancholic, late-night solitude".into());
        p.global.arrangement_hook = Some("recurring 4-note guzheng motif".into());
        p.sections[0].positive_local = vec!["guzheng solo plucking, rain ambience".into()];
        p.sections[1].subtraction = Some("no bass on first 2 bars".into());
        let r = render(&p);
        assert!(
            r.style
                .starts_with("intimate melancholic, late-night solitude, chinese folk ballad"),
            "{}",
            r.style
        );
        assert!(
            r.style.contains("recurring 4-note guzheng motif"),
            "{}",
            r.style
        );
        // Intro positive_local already mentions guzheng, so bare "guzheng" is not duplicated
        assert!(
            r.lyrics.starts_with(
                "[Intro | guzheng solo plucking, rain ambience, energy 1, instrumental]"
            ),
            "{}",
            r.lyrics
        );
        assert!(
            r.lyrics.contains("bass on first 2 bars drop out"),
            "{}",
            r.lyrics
        );
    }
}
