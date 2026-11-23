#[cfg(feature = "desktop")]
use list_rs::{editing, read_xml, sheet::Sheet};
#[cfg(feature = "desktop")]
use std::path::PathBuf;

#[cfg(not(feature = "desktop"))]
fn main() {}

#[cfg(feature = "desktop")]
fn main() {
    let mut args = std::env::args_os().skip(1);
    let file = PathBuf::from(args.next().unwrap());
    let destination = PathBuf::from(args.next().unwrap());
    let original = read_xml(&file).unwrap();
    let sheet = Sheet::import(&original).unwrap();
    let index = sheet
        .regions
        .iter()
        .position(|region| region.source_id == "110")
        .unwrap();
    let source = sheet.regions[index].visual.editor.as_ref().unwrap();
    let replacement = source
        .replacement(&original, "r := sqrt(a^2 + b^2) + 1")
        .unwrap();
    let modified =
        editing::patch(&original, vec![(source.statement.clone(), replacement)]).unwrap();
    let export = editing::export_xml(&modified, true).unwrap();
    let mut reloaded = Sheet::import(&export).unwrap();
    reloaded.full();
    println!("{}", reloaded.display(index).unwrap());
    editing::write_copy(&destination, &export).unwrap();
}
