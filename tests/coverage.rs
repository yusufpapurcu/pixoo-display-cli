use std::{collections::BTreeSet, fs, path::Path};

#[test]
fn every_official_command_page_has_a_real_api_and_fixture_mapping() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rows: serde_json::Value =
        serde_json::from_str(include_str!("../docs/coverage.json")).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 91);
    let mut pages = BTreeSet::new();
    let mut opcodes = BTreeSet::new();
    let sources = [
        include_str!("../src/commands/system.rs"),
        include_str!("../src/commands/schedule.rs"),
        include_str!("../src/commands/tools.rs"),
        include_str!("../src/commands/display.rs"),
    ]
    .join("\n");
    for row in rows {
        let page = row["page"].as_u64().unwrap();
        assert!(pages.insert(page), "duplicate page {page}");
        let snapshot: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join(format!("docs/official/{page}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot["page_id"].as_str().unwrap(), page.to_string());
        assert_eq!(row["title"], snapshot["page_title"]);
        opcodes.insert(row["opcode"].as_str().unwrap());
        let api = row["api"].as_str().unwrap();
        let family = api.split(';').next().unwrap();
        let last = family.rsplit("::").next().unwrap();
        let symbol = last.split([' ', '{', '(', '/']).next().unwrap();
        if !symbol.is_empty() {
            assert!(sources.contains(symbol), "missing API {api}");
        }
        let tests = row["tests"].as_str().unwrap();
        let (file, names) = tests.split_once("::").unwrap();
        let test_source = fs::read_to_string(root.join(file)).unwrap();
        for name in names.split(';').map(str::trim) {
            assert!(
                test_source.contains(&format!("fn {name}(")),
                "missing fixture {name} for page {page}"
            );
        }
        if let Some(workflow) = row["workflow_tests"].as_str().filter(|s| !s.is_empty()) {
            let (file, names) = workflow.split_once("::").unwrap();
            let source = fs::read_to_string(root.join(file)).unwrap();
            for name in names.split(';').map(str::trim) {
                assert!(
                    source.contains(&format!("fn {name}(")),
                    "missing workflow fixture {name}"
                );
            }
        }
        assert_eq!(row["snapshot_sha256"].as_str().unwrap().len(), 64);
        assert!(!row["evidence"].as_str().unwrap().is_empty());
    }
    assert_eq!(opcodes.len(), 90);
    let saved = fs::read_dir(root.join("docs/official"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .count();
    assert_eq!(saved, 92);
}
