//! Living strokes commands (RFC 0017): the actions of the Strokes window as
//! JSON, shared by the agent's `edit_strokes` tool and the plugin API's
//! `stroke_command`, so both say the same things the same way.
//!
//! Every command acts on the active piece. Strokes and groups are named by
//! the ids `list_strokes` / `strokes_state` report.
//!
//! ```json
//! {"op": "set_strength", "strokes": [3, 4], "strength": 0.5}
//! {"op": "delete", "strokes": [3]}
//! {"op": "solo", "strokes": [3]}          {"op": "show_all"}
//! {"op": "keep_unmasked", "strokes": [3]}
//! {"op": "group", "strokes": [3, 4], "name": "Wrinkles"}
//! {"op": "add_to_group", "strokes": [5], "group": 1}
//! {"op": "remove_from_group", "strokes": [5]}
//! {"op": "group_strength", "group": 1, "strength": 0.6}
//! {"op": "group_visible", "group": 1, "visible": false}
//! {"op": "rename_group", "group": 1, "name": "Scales"}
//! {"op": "merge_groups", "from": 2, "into": 1}
//! {"op": "ungroup", "group": 1}
//! {"op": "delete_group", "group": 1}
//! {"op": "bake"}
//! ```

use serde_json::Value;

/// One Strokes window action.
#[derive(Debug, Clone, PartialEq)]
pub enum StrokeOp {
    SetStrength { strokes: Vec<u64>, strength: f32 },
    Delete { strokes: Vec<u64> },
    Solo { strokes: Vec<u64> },
    ShowAll,
    KeepUnmasked { strokes: Vec<u64> },
    Group { strokes: Vec<u64>, name: Option<String> },
    AddToGroup { strokes: Vec<u64>, group: u32 },
    RemoveFromGroup { strokes: Vec<u64> },
    GroupStrength { group: u32, strength: f32 },
    GroupVisible { group: u32, visible: bool },
    RenameGroup { group: u32, name: String },
    MergeGroups { from: u32, into: u32 },
    Ungroup { group: u32 },
    DeleteGroup { group: u32 },
    Bake,
}

/// Strength range the Strokes window allows (0 to 150%).
pub const MAX_STRENGTH: f32 = 1.5;

/// Read a command (see the module docs).
pub fn parse_stroke_op(v: &Value) -> Result<StrokeOp, String> {
    let op = v.get("op").and_then(Value::as_str).ok_or("a stroke command needs \"op\"")?;
    let strokes = || -> Result<Vec<u64>, String> {
        let list = v.get("strokes").and_then(Value::as_array).ok_or(format!("\"{op}\" needs \"strokes\": [ids]"))?;
        let ids: Vec<u64> =
            list.iter().map(|x| x.as_u64().ok_or("stroke ids are whole numbers")).collect::<Result<_, _>>()?;
        if ids.is_empty() {
            return Err(format!("\"{op}\" needs at least one stroke id"));
        }
        Ok(ids)
    };
    let group = |key: &str| -> Result<u32, String> {
        v.get(key)
            .and_then(Value::as_u64)
            .and_then(|g| u32::try_from(g).ok())
            .ok_or(format!("\"{op}\" needs \"{key}\": a group id"))
    };
    let strength = || -> Result<f32, String> {
        let s = v.get("strength").and_then(Value::as_f64).ok_or(format!("\"{op}\" needs \"strength\" (0 to 1.5)"))?;
        if !(0.0..=MAX_STRENGTH as f64).contains(&s) {
            return Err(format!("strength {s} is outside 0 to {MAX_STRENGTH}"));
        }
        Ok(s as f32)
    };
    let name = |required: bool| -> Result<Option<String>, String> {
        match v.get("name").and_then(Value::as_str).map(str::trim) {
            Some(n) if !n.is_empty() => Ok(Some(n.chars().take(80).collect())),
            _ if required => Err(format!("\"{op}\" needs a \"name\"")),
            _ => Ok(None),
        }
    };
    Ok(match op {
        "set_strength" => StrokeOp::SetStrength { strokes: strokes()?, strength: strength()? },
        "delete" => StrokeOp::Delete { strokes: strokes()? },
        "solo" => StrokeOp::Solo { strokes: strokes()? },
        "show_all" => StrokeOp::ShowAll,
        "keep_unmasked" => StrokeOp::KeepUnmasked { strokes: strokes()? },
        "group" => StrokeOp::Group { strokes: strokes()?, name: name(false)? },
        "add_to_group" => StrokeOp::AddToGroup { strokes: strokes()?, group: group("group")? },
        "remove_from_group" => StrokeOp::RemoveFromGroup { strokes: strokes()? },
        "group_strength" => StrokeOp::GroupStrength { group: group("group")?, strength: strength()? },
        "group_visible" => StrokeOp::GroupVisible {
            group: group("group")?,
            visible: v
                .get("visible")
                .and_then(Value::as_bool)
                .ok_or("\"group_visible\" needs \"visible\": true or false")?,
        },
        "rename_group" => StrokeOp::RenameGroup { group: group("group")?, name: name(true)?.unwrap_or_default() },
        "merge_groups" => StrokeOp::MergeGroups { from: group("from")?, into: group("into")? },
        "ungroup" => StrokeOp::Ungroup { group: group("group")? },
        "delete_group" => StrokeOp::DeleteGroup { group: group("group")? },
        "bake" => StrokeOp::Bake,
        other => return Err(format!("unknown stroke command \"{other}\"")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn commands_parse_and_bad_ones_say_why() {
        assert_eq!(
            parse_stroke_op(&json!({"op": "set_strength", "strokes": [3, 4], "strength": 0.5})).unwrap(),
            StrokeOp::SetStrength { strokes: vec![3, 4], strength: 0.5 }
        );
        assert_eq!(
            parse_stroke_op(&json!({"op": "group", "strokes": [1], "name": "  Wrinkles "})).unwrap(),
            StrokeOp::Group { strokes: vec![1], name: Some("Wrinkles".into()) }
        );
        assert_eq!(parse_stroke_op(&json!({"op": "bake"})).unwrap(), StrokeOp::Bake);
        assert_eq!(
            parse_stroke_op(&json!({"op": "merge_groups", "from": 2, "into": 1})).unwrap(),
            StrokeOp::MergeGroups { from: 2, into: 1 }
        );
        for (bad, why) in [
            (json!({}), "needs \"op\""),
            (json!({"op": "fly"}), "unknown stroke command"),
            (json!({"op": "delete"}), "needs \"strokes\""),
            (json!({"op": "delete", "strokes": []}), "at least one"),
            (json!({"op": "set_strength", "strokes": [1], "strength": 3}), "outside"),
            (json!({"op": "rename_group", "group": 1}), "needs a \"name\""),
            (json!({"op": "group_visible", "group": 1}), "visible"),
        ] {
            let e = parse_stroke_op(&bad).unwrap_err();
            assert!(e.contains(why), "{bad}: {e}");
        }
    }
}
