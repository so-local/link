use super::*;

#[test]
fn read_receipts_stay_structured_while_the_model_receives_document_text() {
    let root = link_test_kit::TempDir::new("memory-read-receipt");
    std::fs::create_dir_all(root.join("skills/example")).unwrap();
    let body = "---\nname: example\ndescription: An example.\n---\nRead the facts.\n";
    std::fs::write(root.join("skills/example/SKILL.md"), body).unwrap();
    let index = Index::open(&root, None).unwrap();
    let result = answer(
        &index,
        &json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "read", "arguments": {"path": "skills/example/SKILL.md"}},
        })
        .to_string(),
    )
    .unwrap();
    let structured = &result["result"]["structuredContent"];
    let receipt: link_files::reading::ReadReceipt =
        serde_json::from_value(structured["read"].clone()).unwrap();
    let model: Value = serde_json::from_str(&link_untrusted::unfenced(
        result["result"]["content"][0]["text"].as_str().unwrap(),
    ))
    .unwrap();
    // The receipt names the text the model was given, inside its fence.
    receipt.validate(model["text"].as_str().unwrap()).unwrap();
    assert!(receipt.covers_source());
    assert!(model.get("read").is_none());
    assert_eq!(model["text"], body.trim_end());
    // What the document says is the fenced text's alone.
    for said in ["text", "title"] {
        assert!(structured.get(said).is_none(), "{structured}");
    }
    let mut expected = model.clone();
    let expected = expected.as_object_mut().unwrap();
    expected.remove("text");
    expected.remove("title");
    expected.insert("read".into(), structured["read"].clone());
    assert_eq!(&Value::Object(expected.clone()), structured);
}
