//! Preserve reading order while placing each page under its nearest address ancestor.

use super::Node;

pub(super) fn normalize(nodes: &mut Vec<Node>, site: &str) {
    fn flatten(nodes: Vec<Node>, parent: Option<usize>, out: &mut Vec<(Node, Option<usize>)>) {
        for mut node in nodes {
            let children = std::mem::take(&mut node.children);
            let index = out.len();
            out.push((node, parent));
            flatten(children, Some(index), out);
        }
    }
    let mut flat = Vec::new();
    flatten(std::mem::take(nodes), None, &mut flat);
    let internal = |address: &str| {
        address == site
            || address
                .strip_prefix(site)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    let parents: Vec<_> = flat
        .iter()
        .map(|(node, original)| {
            let Some(address) = node.address.as_deref().filter(|a| internal(a)) else {
                // External links keep their authored position and parent.
                return *original;
            };
            flat.iter()
                .enumerate()
                .filter_map(|(index, (candidate, _))| {
                    let parent = candidate.address.as_deref()?;
                    (parent != site
                        && internal(parent)
                        && address
                            .strip_prefix(parent)
                            .is_some_and(|rest| rest.starts_with('/')))
                    .then_some((parent.len(), index))
                })
                .max_by_key(|(length, _)| *length)
                .map(|(_, index)| index)
                // Under no page but the home page, a page keeps the home
                // page as its parent when the curator put it there.
                .or_else(|| original.filter(|&p| flat[p].0.address.as_deref() == Some(site)))
        })
        .collect();
    let mut children = vec![Vec::new(); flat.len()];
    let mut roots = Vec::new();
    for (index, parent) in parents.into_iter().enumerate() {
        match parent {
            Some(parent) => children[parent].push(index),
            None => roots.push(index),
        }
    }
    fn take(index: usize, flat: &mut [Option<Node>], children: &[Vec<usize>]) -> Node {
        let mut node = flat[index].take().expect("each page has one parent");
        node.children = children[index]
            .iter()
            .map(|&child| take(child, flat, children))
            .collect();
        node
    }
    let mut flat: Vec<_> = flat.into_iter().map(|(node, _)| Some(node)).collect();
    *nodes = roots
        .into_iter()
        .map(|index| take(index, &mut flat, &children))
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(path: &str, children: Vec<Node>) -> Node {
        Node {
            label: path.into(),
            target: path.into(),
            children,
            file: None,
            address: Some(format!("https://docs.example{path}")),
        }
    }

    #[test]
    fn misplaced_and_flat_pages_rejoin_their_address_parent_in_reading_order() {
        let mut outline = vec![
            page("/", vec![]),
            page("/b", vec![page("/a/two", vec![])]),
            page("/a/one", vec![]),
            page("/a", vec![page("/ab", vec![])]),
        ];
        normalize(&mut outline, "https://docs.example");
        assert_eq!(
            outline.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(),
            ["/", "/b", "/a", "/ab"]
        );
        assert_eq!(
            outline[2]
                .children
                .iter()
                .map(|n| n.label.as_str())
                .collect::<Vec<_>>(),
            ["/a/two", "/a/one"]
        );
        let before = format!("{outline:?}");
        normalize(&mut outline, "https://docs.example");
        assert_eq!(format!("{outline:?}"), before);
    }

    #[test]
    fn external_links_retain_their_authored_parent_and_nearest_ancestor_wins() {
        let external = Node {
            address: None,
            ..page("/terms", vec![])
        };
        let mut outline = vec![
            page("/", vec![external]),
            page("/a", vec![page("/a/b/c", vec![])]),
            page("/a/b", vec![]),
        ];
        normalize(&mut outline, "https://docs.example");
        assert_eq!(outline[0].children[0].label, "/terms");
        assert_eq!(outline[1].children[0].label, "/a/b");
        assert_eq!(outline[1].children[0].children[0].label, "/a/b/c");
    }
}
