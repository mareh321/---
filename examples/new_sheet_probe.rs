#[cfg(not(feature = "desktop"))]
fn main() {}

#[cfg(feature = "desktop")]
fn main() {
    use list_rs::editing::{export_xml, insert_region, write_copy, ChartSpec, NewRegion};
    let output = std::path::PathBuf::from(std::env::args_os().nth(1).unwrap());
    let blank = include_str!("blank.xmcd");
    let (xml, _) =
        insert_region(blank, 12., 10., NewRegion::Text("Проверка нового листа")).unwrap();
    let (xml, _) = insert_region(&xml, 12., 45., NewRegion::Formula("a := 2")).unwrap();
    let (xml, _) = insert_region(&xml, 12., 85., NewRegion::Formula("result := a^2 =")).unwrap();
    let data = ChartSpec::Function {
        expression: "a * sin(x); cos(x)".into(),
        from: -6.28,
        to: 6.28,
    };
    let (xml, _) = insert_region(&xml, 12., 130., NewRegion::Chart(&data)).unwrap();
    write_copy(&output, &export_xml(&xml, true).unwrap()).unwrap();
}
