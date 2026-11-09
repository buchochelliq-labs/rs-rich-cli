//! Optional formatting facts absent from DOMSnapshot's layout objects.
use super::*;
use std::collections::{HashMap, HashSet};

const GROUP: &str = "noctiluca-source-formatting";
const GUARD: &str = "__noctilucaSourceFormatting";

pub(super) fn begin(
    call: &impl Fn(&str, Value) -> io::Result<Value>,
    context: i64,
) -> io::Result<()> {
    call(
        "Runtime.evaluate",
        json!({"contextId":context,"expression":format!(r#"(() => {{
      globalThis.{GUARD}?.observer.disconnect();
      const state = {{ document, changed: false }};
      state.observer = new MutationObserver(() => state.changed = true);
      state.observer.observe(document, {{subtree:true,childList:true,attributes:true,characterData:true}});
      globalThis.{GUARD} = state;
    }})()"#)}),
    )?;
    Ok(())
}

// Only missing-box element chains immediately below a known grid are candidates.
// Descendants of a real box belong to its own formatting context. A hard bound
// leaves the snapshot's unknown facts intact on unusually large graphs.
fn candidates(dom: &Value) -> Vec<(i64, i64)> {
    let d = &dom["documents"][0];
    let n = &d["nodes"];
    let Some(parents) = n["parentIndex"].as_array() else {
        return Vec::new();
    };
    let mut boxes = HashSet::new();
    let mut grids = HashSet::new();
    if let Some(indexes) = d["layout"]["nodeIndex"].as_array() {
        for (k, index) in indexes.iter().enumerate() {
            let Some(index) = index.as_u64().map(|v| v as usize) else {
                continue;
            };
            boxes.insert(index);
            let display = d["layout"]["styles"][k][0]
                .as_u64()
                .and_then(|i| dom["strings"][i as usize].as_str());
            if matches!(display, Some("grid" | "inline-grid")) {
                grids.insert(index);
            }
        }
    }
    let mut open = grids;
    let mut result = Vec::new();
    for (i, parent) in parents.iter().enumerate() {
        let Some(parent) = parent.as_u64().map(|p| p as usize) else {
            continue;
        };
        if open.contains(&parent)
            && !boxes.contains(&i)
            && n["nodeType"][i].as_u64() == Some(1)
            && let (Some(node), Some(parent)) = (
                n["backendNodeId"][i].as_i64(),
                n["backendNodeId"][parent].as_i64(),
            )
        {
            result.push((node, parent));
            if result.len() > 128 {
                return Vec::new();
            }
            open.insert(i);
        }
    }
    result
}

pub(super) fn finish(
    call: &impl Fn(&str, Value) -> io::Result<Value>,
    context: i64,
    dom: &mut Value,
) {
    let read = (|| -> io::Result<Value> {
        let pairs = candidates(dom);
        if pairs.is_empty() {
            return Ok(Value::Null);
        }
        let document = dom["documents"][0]["nodes"]["backendNodeId"][0]
            .as_i64()
            .ok_or_else(|| io::Error::other("no snapshot document"))?;
        let mut objects = HashMap::new();
        for node in std::iter::once(document).chain(pairs.iter().flat_map(|&(n, p)| [n, p])) {
            if objects.contains_key(&node) {
                continue;
            }
            let v = call(
                "DOM.resolveNode",
                json!({"backendNodeId":node,"executionContextId":context,"objectGroup":GROUP}),
            )?;
            let id = v["object"]["objectId"]
                .as_str()
                .ok_or_else(|| io::Error::other("detached formatting node"))?;
            objects.insert(node, id.to_string());
        }
        let args: Vec<_> = pairs
            .iter()
            .flat_map(|(n, p)| {
                [
                    json!({"objectId":objects[n]}),
                    json!({"objectId":objects[p]}),
                ]
            })
            .collect();
        let v = call(
            "Runtime.callFunctionOn",
            json!({"objectId":objects[&document],"arguments":args,"returnByValue":true,
          "functionDeclaration":format!(r#"function(...nodes) {{
            const state = globalThis.{GUARD};
            if (!state || state.document !== this || this !== document || state.changed || state.observer.takeRecords().length) return null;
            return nodes.reduce((out, n, i) => {{
              if (i % 2) return out;
              if (!n.isConnected || n.ownerDocument !== this || n.parentNode !== nodes[i+1]) {{out.push(null);return out;}}
              const s = getComputedStyle(n); out.push([s.display,s.position]);return out;
            }}, []);
          }}"#)}),
        )?;
        let Some(values) = v["result"]["value"]
            .as_array()
            .filter(|v| v.len() == pairs.len())
        else {
            return Ok(Value::Null);
        };
        Ok(Value::Array(
            pairs
                .iter()
                .zip(values)
                .filter_map(|(&(node, _), v)| {
                    Some(json!({"node":node,"display":v[0].as_str()?,"position":v[1].as_str()?}))
                })
                .collect(),
        ))
    })();
    let _ = call("Runtime.releaseObjectGroup", json!({"objectGroup":GROUP}));
    let _ = call(
        "Runtime.evaluate",
        json!({"contextId":context,"expression":format!("globalThis.{GUARD}?.observer.disconnect(); delete globalThis.{GUARD}")}),
    );
    if let Ok(Value::Array(facts)) = read {
        dom["sourceFormatting"] = Value::Array(facts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_candidates_are_only_boxless_grid_chains() {
        let dom: Value =
            serde_json::from_str(include_str!("../../tests/snapshots/source-native/dom.json"))
                .unwrap();
        assert_eq!(
            candidates(&dom),
            vec![(91, 87), (101, 87), (111, 87), (123, 119), (133, 119)]
        );
    }
    #[test]
    fn uncertain_or_detached_reads_keep_facts_absent_and_release_handles() {
        use std::cell::RefCell;
        for detached in [false, true] {
            let calls = RefCell::new(Vec::new());
            let mut dom: Value =
                serde_json::from_str(include_str!("../../tests/snapshots/source-native/dom.json"))
                    .unwrap();
            finish(
                &|method, _args| {
                    calls.borrow_mut().push(method.to_string());
                    match method {
                        "DOM.resolveNode" if detached => Err(io::Error::other("gone")),
                        "DOM.resolveNode" => Ok(json!({"object":{"objectId":"resolved"}})),
                        "Runtime.callFunctionOn" => Ok(json!({"result":{"value":null}})),
                        _ => Ok(json!({})),
                    }
                },
                1,
                &mut dom,
            );
            assert!(dom.get("sourceFormatting").is_none());
            assert!(
                calls
                    .borrow()
                    .iter()
                    .any(|m| m == "Runtime.releaseObjectGroup")
            );
            assert_eq!(calls.borrow().last().unwrap(), "Runtime.evaluate");
        }
    }
}
