fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = arboard::Clipboard::new()?.get_text()?;
    println!("{text}");
    Ok(())
}
