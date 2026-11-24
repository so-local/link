//! A reference page: written whole, however long, since what it lists is
//! looked up by its headings, never read through.

use std::path::Path;

use crate::Files;

/// One section: its heading and its body.
#[derive(Clone)]
pub(super) struct Section {
    pub heading: String,
    pub body: String,
}

/// A page's title, its opening and its sections.
pub(super) struct Page {
    pub title: String,
    pub opening: String,
    pub sections: Vec<Section>,
    /// One heading over every section's body in place of a heading each,
    /// and the sentence opening it.
    pub merged: Option<(String, String)>,
    /// The sections before the others: each heading and its body.
    pub preface: Vec<(String, String)>,
}

impl Page {
    /// The page, written at `path`.
    pub fn write(self, path: &Path, files: &mut Files) -> anyhow::Result<()> {
        files.insert(path, self.render())
    }

    fn render(&self) -> String {
        let mut out = format!("{}# {}\n\n{}\n", header(), self.title, self.opening);
        for (heading, body) in &self.preface {
            out.push_str(&format!("\n## {heading}\n\n{}\n", body.trim_end()));
        }
        if let Some((heading, opening)) = &self.merged {
            out.push_str(&format!("\n## {heading}\n\n{opening}\n\n"));
            for section in &self.sections {
                out.push_str(&section.body);
            }
            return format!("{}\n", out.trim_end());
        }
        for section in &self.sections {
            out.push_str(&format!(
                "\n## {}\n\n{}\n",
                section.heading,
                section.body.trim_end()
            ));
        }
        out
    }
}

fn header() -> String {
    format!("<!-- {} -->\n", crate::marker())
}
