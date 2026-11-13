use chronobreak::{Direction, EditUnit, TextEditModel};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut model = TextEditModel::new("你好 e\u{301} 👩🏽‍💻");
    model.move_cursor(Direction::Backward, EditUnit::Grapheme, true);
    assert_eq!(model.selected_text(), "👩🏽‍💻");
    model.edit_group(|model| {
        model.replace_selection("🇨🇳");
        model.replace_selection("!");
    });
    assert_eq!(model.text(), "你好 e\u{301} 🇨🇳!");
    assert!(model.undo());
    assert_eq!(model.selected_text(), "👩🏽‍💻");
    assert!(model.redo());
    model.set_selection(0, "你好".len())?;
    println!(
        "{} | selected: {} | revision: {}",
        model.text(),
        model.selected_text(),
        model.revision()
    );
    Ok(())
}
