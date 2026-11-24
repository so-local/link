//! How good search is, as a number a change can't lower: a fixed folder
//! of notes and conversations, the kind an agent's files hold, with
//! their dates, and questions put the way a model puts them, each with
//! the document it should find first. By words: `cargo test` has no
//! embedding model.
//!
//! A ranking change that finds fewer first, or shows a copy of a
//! document beside it, fails here. One that finds more raises
//! [`FIRST`] in the same change, so the next can't give it back. `-- --nocapture` shows each question's result.

use std::path::Path;

use link_memory_tool::{Filter, Index, parse_since};

/// How many of [`QUESTIONS`] find their document first, at least.
const FIRST: usize = 15;

/// The folder: each document's path, the date it last changed, its text.
const FOLDER: &[(&str, &str, &str)] = &[
    (
        "memory/notes/garden.md",
        "2026-03-01",
        "# Garden\n\nPlant the tomatoes after the last frost, mid May. Basil goes beside \
         them. Water deeply twice a week.\n",
    ),
    (
        "memory/notes/passport.md",
        "2025-11-02",
        "# Passport renewal\n\nForm DS-82, two photos, the old passport and a check for \
         $130, mailed to the Philadelphia address. It takes six to eight weeks.\n",
    ),
    (
        "memory/notes/travel-checklist.md",
        "2026-01-10",
        "# Travel checklist\n\nChargers, passport, toothbrush, the blue jacket, snacks for \
         the plane.\n",
    ),
    (
        "memory/notes/budget-2025.md",
        "2025-01-15",
        "# Budget\n\nOur household budget: rent 2,100, groceries 600, car 350.\n",
    ),
    (
        "memory/notes/budget-2026.md",
        "2026-01-15",
        "# Budget\n\nOur household budget: rent 2,250, groceries 650, car 350, daycare \
         1,400.\n",
    ),
    (
        "memory/notes/recipes/lemon-cake.md",
        "2025-06-01",
        "# Lemon cake\n\nThree lemons, zest and juice. 200 g sugar, 3 eggs, 180 g flour, \
         butter. Bake 40 minutes at 180 C.\n",
    ),
    (
        "memory/notes/recipes/lemon-cake copy.md",
        "2025-06-01",
        "# Lemon cake\n\nThree lemons, zest and juice. 200 g sugar, 3 eggs, 180 g flour, \
         butter. Bake 40 minutes at 180 C.\n",
    ),
    (
        "memory/notes/recipes/chili.md",
        "2025-02-11",
        "# Chili\n\nBeans, ground beef, cumin, two onions, a can of tomatoes. Simmer an \
         hour.\n",
    ),
    (
        "memory/notes/people/maya.md",
        "2025-08-20",
        "# Maya\n\nBirthday March 14. Likes lemon cake, hates chocolate. Allergic to \
         peanuts.\n",
    ),
    (
        "memory/notes/people/sam.md",
        "2026-02-03",
        "# Sam\n\nFrom the climbing gym. Works night shifts at the hospital. Owes me 40 \
         dollars for the concert tickets.\n",
    ),
    (
        "memory/notes/car.md",
        "2026-08-30",
        "# Car\n\nSubaru Outback, 2019. Oil change every 6,000 miles; the last at 48,200. \
         Tires rotated in August.\n",
    ),
    (
        "memory/notes/wifi.txt",
        "2024-05-05",
        "Home wifi: the network is LinkHome, and the password is on the fridge magnet.\n",
    ),
    (
        "memory/notes/doctor.md",
        "2025-10-12",
        "# Doctor\n\nDr. Alvarez, family medicine. Annual physical in October. Blood \
         pressure was fine last time.\n",
    ),
    (
        "memory/notes/books.md",
        "2026-04-18",
        "# Books to read\n\nThe Overstory, Piranesi, Project Hail Mary.\n",
    ),
    (
        "memory/notes/work/standup-2026-09-01.md",
        "2026-09-01",
        "# Standup\n\nShipped the login page. Blocked on the payments API keys.\n",
    ),
    (
        "memory/notes/work/standup-2026-09-22.md",
        "2026-09-22",
        "# Standup\n\nThe payments API keys arrived. Login page bug fixed. Next: \
         invoices.\n",
    ),
    (
        "memory/notes/insurance.md",
        "2026-04-02",
        "# Home insurance\n\nPolicy HX-99812 with Northwind. Renews every April. \
         Deductible 1,000.\n",
    ),
    (
        "memory/notes/plumber.md",
        "2026-07-19",
        "# Plumber\n\nJoe's Plumbing, 555-0142. Fixed the kitchen sink leak in July.\n",
    ),
    (
        "memory/notes/gym.md",
        "2026-05-06",
        "# Climbing\n\nThe gym opens at 6. Bouldering on Tuesdays.\n",
    ),
    (
        "memory/notes/taxes.md",
        "2026-02-20",
        "# Taxes\n\nReceipts go in the blue folder. The accountant is Priya; they're due \
         April 15.\n",
    ),
    (
        "memory/notes/school.md",
        "2026-08-25",
        "# School\n\nPickup at 3:15, early release Wednesdays at 1:00. The teacher is Ms. \
         Okafor.\n",
    ),
    (
        "chats/telegram-42/conversation.md",
        "2026-03-01",
        "# telegram:42\n\n## You, 2026-03-01T09:00:00Z\n\nThe gate code at the cabin is \
         4729.\n\n## Agent, 2026-03-01T09:00:02Z\n\nNoted.\n",
    ),
    (
        "chats/slack-D0123ABCDEF/conversation.md",
        "2026-06-02",
        "# slack:D0123ABCDEF\n\n## You, 2026-06-02T18:04:11Z\n\nCan you book me a rental car \
         for Albuquerque?\n\n## Agent, 2026-06-02T18:04:19Z\n\nBooked a blue Subaru from \
         Sunny Rentals. The confirmation is SR-4471.\n",
    ),
    (
        "chats/telegram-7/conversation.md",
        "2026-09-10",
        "# telegram:7\n\n## You, 2026-09-10T20:00:00Z\n\nMaya's party is on Saturday at \
         the park. I'll bring the lemon cake.\n\n## Agent, 2026-09-10T20:00:03Z\n\nI'll \
         remind you on Friday.\n",
    ),
];

/// Questions as a model puts them, and the document each should find
/// first.
const QUESTIONS: &[(&str, &str)] = &[
    (
        "When should I plant the tomatoes?",
        "memory/notes/garden.md",
    ),
    ("How do I renew my passport?", "memory/notes/passport.md"),
    (
        "What's our household budget this year?",
        "memory/notes/budget-2026.md",
    ),
    ("lemon cake recipe", "memory/notes/recipes/lemon-cake.md"),
    ("What is Maya allergic to?", "memory/notes/people/maya.md"),
    ("How much does Sam owe me?", "memory/notes/people/sam.md"),
    ("When is the next oil change due?", "memory/notes/car.md"),
    ("wifi password", "memory/notes/wifi.txt"),
    (
        "Are the payments API keys still blocking us?",
        "memory/notes/work/standup-2026-09-22.md",
    ),
    (
        "What's the gate code at the cabin?",
        "chats/telegram-42/conversation.md",
    ),
    (
        "rental car confirmation number",
        "chats/slack-D0123ABCDEF/conversation.md",
    ),
    ("When is Maya's party?", "chats/telegram-7/conversation.md"),
    ("Who is our accountant?", "memory/notes/taxes.md"),
    ("who fixed the kitchen sink", "memory/notes/plumber.md"),
    ("What time is school pickup?", "memory/notes/school.md"),
    ("home insurance policy number", "memory/notes/insurance.md"),
];

fn write(root: &Path, path: &str, date: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, text).unwrap();
    let secs = parse_since(date).unwrap() as u64;
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .unwrap();
}

/// A document's text, to tell a copy from another document.
fn text_of(path: &str) -> &'static str {
    FOLDER.iter().find(|(p, _, _)| *p == path).unwrap().2
}

#[test]
fn search_finds_what_it_should_first() {
    let root = link_test_kit::TempDir::new("quality");
    for (path, date, text) in FOLDER {
        write(&root, path, date, text);
    }
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();

    let (mut first, mut copies) = (0, 0);
    for (question, expected) in QUESTIONS {
        let hits = index.search(question, Filter::default(), 3).unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        // A copy of the document found first is the document.
        let found = paths
            .first()
            .is_some_and(|p| text_of(p) == text_of(expected));
        let copied = paths
            .iter()
            .enumerate()
            .any(|(i, a)| paths[i + 1..].iter().any(|b| text_of(a) == text_of(b)));
        first += usize::from(found);
        copies += usize::from(copied);
        println!(
            "{} {question:?} -> {}{}",
            if found { "ok  " } else { "MISS" },
            paths.join(", "),
            if copied { "  (a copy twice)" } else { "" }
        );
    }
    println!(
        "{first} of {} first; {copies} with a copy twice",
        QUESTIONS.len()
    );
    assert!(
        first >= FIRST,
        "search found {first} of {} first, fewer than {FIRST}",
        QUESTIONS.len()
    );
    // A copy comes once, in the `copies` of the one shown.
    assert_eq!(copies, 0, "{copies} questions showed a copy twice");
}
