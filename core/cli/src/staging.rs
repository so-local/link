//! List old staging in data without changing the person's files.

use link_format::{Done, Out, Report, Table, WithinExt, sentence};

pub fn run(out: &mut Out, asked: &crate::cli::FolderCheckArgs) -> Result<Done, Report> {
    let files = link_files::maintenance::staged_data(link_files::maintenance::now()?)
        .within("listing old staging in your data")?;
    if asked.json {
        out.reply(&files);
    } else {
        let mut table = Table::new(
            ["Old staging file"],
            sentence!("No old staging files were found in your data."),
        );
        for file in files {
            table.row([file.display().to_string()]);
        }
        out.block(table);
    }
    Ok(Done::shown())
}
