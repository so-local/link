use super::super::*;

/// A call's arguments are held to their declaration before the index is
/// asked: an undeclared choice, a pattern broken, a wrong type, and an
/// argument not declared are each refused, naming it.
#[test]
fn a_call_breaking_its_declaration_is_refused() {
    let root = link_test_kit::TempDir::new("memory-declared");
    let index = Index::open(&root, None).unwrap();
    for (arguments, says) in [
        (
            json!({"query": "notes", "kind": "photo"}),
            "unknown variant `photo`",
        ),
        (json!({"query": "notes", "since": "last week"}), "since"),
        (json!({"query": 5}), "invalid type"),
        (
            json!({"query": "notes", "deep": true}),
            "unknown field `deep`",
        ),
    ] {
        let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "search", "arguments": arguments}});
        let reply = answer(&index, &call.to_string()).unwrap();
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains(says), "{text}");
    }
    // A number of another type is refused by the handler too, never
    // taken as left out.
    let why = call(index.reader(), tokens_max(), "list", &json!({"limit": "5"})).unwrap_err();
    assert!(
        why.to_string().contains("'limit' is a whole number"),
        "{why}"
    );
}

/// `list` and `links`, cut by their size or their limit, say `next`,
/// the `page` or `skip` that carries on, and following it gives each
/// once; a search's pages give each once, up to the most it ranks.
#[test]
fn a_cut_answer_says_where_to_carry_on() {
    let root = link_test_kit::TempDir::new("memory-next");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    let mut hub = String::from("# Hub\n\n");
    for n in 0..60 {
        std::fs::write(
            root.join(format!("memory/note-{n:02}.md")),
            format!("Note {n} about the garden.\n"),
        )
        .unwrap();
        hub.push_str(&format!("[note {n}](note-{n:02}.md)\n"));
    }
    std::fs::write(root.join("memory/hub.md"), hub).unwrap();
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    // A list, page by page.
    let mut seen = Vec::new();
    let mut next: Option<String> = None;
    loop {
        let page = call(
            index.reader(),
            tokens_max(),
            "list",
            &json!({"limit": 7, "page": next}),
        )
        .unwrap();
        for document in page["documents"].as_array().unwrap() {
            seen.push(document["path"].as_str().unwrap().to_string());
        }
        match page["next"].as_str() {
            Some(page) => next = Some(page.to_string()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 61);
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 61, "each once");
    // A hub's links, cut by a small size.
    let mut links = Vec::new();
    let mut skip = 0;
    loop {
        let page = call(
            index.reader(),
            tokens_max(),
            "links",
            &json!({"path": "memory/hub.md", "tokens": 60, "skip": skip}),
        )
        .unwrap();
        for link in page["to"].as_array().unwrap() {
            links.push(link["path"].as_str().unwrap().to_string());
        }
        match page["next"].as_u64() {
            Some(next) => {
                assert_eq!(page["cut"], true);
                skip = next as usize;
            }
            None => break,
        }
    }
    links.sort();
    links.dedup();
    assert_eq!(links.len(), 60);
    // A search, page by page: words alone rank at most 50, so the last
    // page has no `next`, and says more match.
    let mut found = Vec::new();
    let mut next: Option<String> = None;
    let last = loop {
        let page = call(
            index.reader(),
            tokens_max(),
            "search",
            &json!({"query": "garden", "limit": 20, "page": next}),
        )
        .unwrap();
        for hit in page["results"].as_array().unwrap() {
            found.push(hit["path"].as_str().unwrap().to_string());
        }
        match page["next"].as_str() {
            Some(page) => next = Some(page.to_string()),
            None => break page,
        }
    };
    assert_eq!(found.len(), 50, "{found:?}");
    found.sort();
    found.dedup();
    assert_eq!(found.len(), 50, "each once");
    assert_eq!(last["more"], PAST_SEARCH, "{last}");
    // A list's page is no search's.
    let list = call(index.reader(), tokens_max(), "list", &json!({"limit": 1})).unwrap();
    let wrong = call(
        index.reader(),
        tokens_max(),
        "search",
        &json!({"query": "garden", "page": list["next"]}),
    );
    assert!(wrong.is_err(), "{wrong:?}");
}

/// What was removed comes only with the first page, so it's fitted
/// first, and the documents after it, at least one, so `next` moves on.
#[test]
fn a_lists_removed_documents_are_fitted_first() {
    let root = link_test_kit::TempDir::new("memory-removed-first");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    for n in 0..4 {
        std::fs::write(
            root.join(format!("memory/note-{n}.md")),
            format!("Note {n}.\n"),
        )
        .unwrap();
    }
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    std::fs::remove_file(root.join("memory/note-0.md")).unwrap();
    std::fs::remove_file(root.join("memory/note-1.md")).unwrap();
    index.refresh().unwrap();
    let page = call(
        index.reader(),
        tokens_max(),
        "list",
        &json!({"since": "1970-01-01", "tokens": 1}),
    )
    .unwrap();
    assert_eq!(page["removed"].as_array().unwrap().len(), 1, "{page}");
    assert_eq!(page["documents"].as_array().unwrap().len(), 1, "{page}");
    assert_eq!(page["cut"], json!(true));
    // The page after holds the rest, and nothing removed.
    let rest = call(
        index.reader(),
        tokens_max(),
        "list",
        &json!({"since": "1970-01-01", "page": page["next"]}),
    )
    .unwrap();
    assert!(rest.get("removed").is_none(), "{rest}");
    assert_eq!(rest["documents"].as_array().unwrap().len(), 1, "{rest}");
}
