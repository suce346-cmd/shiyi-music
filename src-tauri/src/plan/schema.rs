//! SongPlan / PlanIR 的 JSON Schema（P1-1）：schemars 生成 → strict 后处理 → 快照锁定。
//! 用途：P1-2 `call_llm_structured` 的 `response_format.json_schema`，以及本地二次校验
//! （国产网关可能忽略 `strict`——§2.2 风险项，本地校验必做）。

use super::{PlanIr, SongPlan, SCHEMA_VERSION};
use serde_json::Value;

pub const SONG_PLAN_SCHEMA_NAME: &str = "song_plan";
pub const PLAN_IR_SCHEMA_NAME: &str = "plan_ir";

/// OpenAI `json_schema strict` 兼容后处理：
/// - 每个带 `properties` 的对象：全部属性 required（Option 已生成为可 null）、`additionalProperties:false`；
/// - 去掉 strict 子集不支持的 `format`（如 uint8）；数值区间保留 minimum/maximum 供本地校验；
/// - 去掉 `description`：Rust 文档注释含内部引用（§/P1-x/文件名），对模型是噪声且增加 token；
///   字段语义由 P1-3 提示词单独说明（schema 只管形状与取值域）。
/// OpenAI strict 兼容后处理。
/// - 只剥**schema 元键** `description`/`format`（与 `type`/`properties` 同级的那些）。
/// - **绝不**把 `properties` 映射本身当 schema 递归剥键——否则名为 `description`/`format`
///   的字段定义会被删掉（模型再也填不了这两个字段名）。
fn strictify(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.remove("format");
            if map.get("description").is_some_and(|d| d.is_string()) {
                map.remove("description");
            }
            if let Some(Value::Object(props)) = map.get("properties") {
                let keys: Vec<Value> = props.keys().cloned().map(Value::String).collect();
                map.insert("required".into(), Value::Array(keys));
                map.insert("additionalProperties".into(), Value::Bool(false));
            }
            // properties 映射：只递归其**值**（子 schema），不把映射自身当 schema
            if let Some(Value::Object(props)) = map.get_mut("properties") {
                for child in props.values_mut() {
                    strictify(child);
                }
            }
            for (k, child) in map.iter_mut() {
                if k == "properties" {
                    continue;
                }
                strictify(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strictify),
        _ => {}
    }
}

fn generate<T: schemars::JsonSchema>() -> Value {
    let mut v = serde_json::to_value(schemars::schema_for!(T)).expect("schema 序列化");
    strictify(&mut v);
    v
}

pub fn song_plan_schema() -> Value {
    generate::<SongPlan>()
}

pub fn plan_ir_schema() -> Value {
    let mut v = generate::<PlanIr>();
    if let Value::Object(m) = &mut v {
        m.insert("title".into(), Value::String("PlanIr".into()));
    }
    v
}

/// 结构化输出解析失败的单条错误（JSON 路径 + 原因），供 Failed{kind=Parse, detail} 使用（§2.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSchemaError {
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for PlanSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", if self.path.is_empty() { "/" } else { &self.path }, self.message)
    }
}

pub(crate) fn check(schema: &Value, instance: &Value) -> Result<(), Vec<PlanSchemaError>> {
    let validator = jsonschema::validator_for(schema).map_err(|e| {
        vec![PlanSchemaError { path: String::new(), message: format!("schema 无效: {e}") }]
    })?;
    let errors: Vec<PlanSchemaError> = validator
        .iter_errors(instance)
        .map(|e| PlanSchemaError { path: e.instance_path().to_string(), message: e.to_string() })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// 解析模型产出的 SongPlan：先 schema 校验（拿到全部路径级错误），再反序列化。
/// 失败不降级、不自由文本兜底（RULE-STRUCTURED-ONLY）。
pub fn parse_song_plan(instance: &Value) -> Result<SongPlan, Vec<PlanSchemaError>> {
    check(&song_plan_schema(), instance)?;
    let plan: SongPlan = serde_json::from_value(instance.clone())
        .map_err(|e| vec![PlanSchemaError { path: String::new(), message: e.to_string() }])?;
    debug_assert_eq!(plan.schema_version, SCHEMA_VERSION);
    Ok(plan)
}

pub fn parse_plan_ir(instance: &Value) -> Result<PlanIr, Vec<PlanSchemaError>> {
    check(&plan_ir_schema(), instance)?;
    serde_json::from_value(instance.clone())
        .map_err(|e| vec![PlanSchemaError { path: String::new(), message: e.to_string() }])
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::minimal_plan;
    use super::*;

    const SNAP_SONG_PLAN: &str = include_str!("../../tests/fixtures/schema/song_plan.v1.json");
    const SNAP_PLAN_IR: &str = include_str!("../../tests/fixtures/schema/plan_ir.v1.json");

    fn pretty(v: &Value) -> String {
        serde_json::to_string_pretty(v).unwrap() + "\n"
    }

    /// 快照锁：schema 变更必须显式更新快照（SHIYI_UPDATE_SNAPSHOTS=1 cargo test --lib plan::schema）
    #[test]
    fn schema_snapshots_byte_identical() {
        let cur = [(pretty(&song_plan_schema()), SNAP_SONG_PLAN, "song_plan.v1.json"), (pretty(&plan_ir_schema()), SNAP_PLAN_IR, "plan_ir.v1.json")];
        if std::env::var("SHIYI_UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
            for (s, _, name) in &cur {
                let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema").join(name);
                std::fs::write(p, s).unwrap();
            }
            return;
        }
        for (s, snap, name) in &cur {
            assert_eq!(s, snap, "{name} 快照不一致（schema 变更需显式更新快照）");
        }
    }

    /// strict 兼容：每个对象 additionalProperties=false 且全部属性 required；无 format 关键字
    #[test]
    fn schema_is_strict_compatible() {
        fn walk(v: &Value, path: &str) {
            if let Value::Object(m) = v {
                assert!(!m.contains_key("format"), "{path} 含 format");
                assert!(!m.get("description").is_some_and(|d| d.is_string()), "{path} 含 description");
                if let Some(Value::Object(props)) = m.get("properties") {
                    assert_eq!(m.get("additionalProperties"), Some(&Value::Bool(false)), "{path}");
                    let req: Vec<&str> = m["required"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
                    for k in props.keys() {
                        assert!(req.contains(&k.as_str()), "{path}.{k} 未 required");
                    }
                }
                for (k, c) in m {
                    walk(c, &format!("{path}/{k}"));
                }
            } else if let Value::Array(a) = v {
                for (i, c) in a.iter().enumerate() {
                    walk(c, &format!("{path}/{i}"));
                }
            }
        }
        walk(&song_plan_schema(), "");
        walk(&plan_ir_schema(), "");
    }

    /// 风险项（schema 过大使国产模型退化）：体积上限守护
    #[test]
    fn schema_size_bounded() {
        let n = serde_json::to_string(&song_plan_schema()).unwrap().len();
        assert!(n < 8000, "song_plan schema {n} 字节，超出精简预算");
    }

    #[test]
    fn parse_valid_plan() {
        let v = serde_json::to_value(minimal_plan()).unwrap();
        assert_eq!(parse_song_plan(&v).unwrap(), minimal_plan());
        assert_eq!(parse_plan_ir(&v).unwrap().0, minimal_plan());
    }

    /// 负路径：错误带 JSON 路径（§2.2 失败语义）
    #[test]
    fn parse_reports_json_paths() {
        let mut v = serde_json::to_value(minimal_plan()).unwrap();
        v["sections"][1]["energy"] = serde_json::json!(11);
        v["sections"][0]["kind"] = serde_json::json!("solo");
        v["settings"]["variety"] = serde_json::json!(101);
        v["schema_version"] = serde_json::json!(2);
        let errs = parse_song_plan(&v).unwrap_err();
        let paths: Vec<&str> = errs.iter().map(|e| e.path.as_str()).collect();
        for want in ["/sections/1/energy", "/sections/0/kind", "/settings/variety", "/schema_version"] {
            assert!(paths.contains(&want), "缺路径 {want}: {errs:?}");
        }
    }

    #[test]
    fn parse_rejects_missing_and_extra_fields() {
        let mut v = serde_json::to_value(minimal_plan()).unwrap();
        v.as_object_mut().unwrap().remove("notes");
        v["extra"] = serde_json::json!(true);
        let errs = parse_song_plan(&v).unwrap_err();
        assert!(errs.len() >= 2, "{errs:?}");
    }

    #[test]
    fn strictify_keeps_properties_named_description_or_format() {
        let mut v = serde_json::json!({
            "type": "object",
            "description": "schema meta — must strip",
            "format": "meta-format",
            "properties": {
                "description": { "type": "string", "description": "field meta — strip" },
                "format": { "type": "string", "format": "uri" },
                "title": { "type": "string" }
            }
        });
        strictify(&mut v);
        let props = v["properties"].as_object().unwrap();
        assert!(props.contains_key("description"), "字段名 description 不得被剥");
        assert!(props.contains_key("format"), "字段名 format 不得被剥");
        assert!(props.contains_key("title"));
        assert!(v.get("description").is_none(), "元键 description 应剥");
        assert!(v.get("format").is_none(), "元键 format 应剥");
        assert!(props["description"].get("description").is_none());
        assert!(props["format"].get("format").is_none());
        let req: Vec<&str> = v["required"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        for k in ["description", "format", "title"] {
            assert!(req.contains(&k), "{k}");
        }
    }
}
