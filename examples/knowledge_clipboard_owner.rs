use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text = String::new();
    std::io::stdin().take(65537).read_to_string(&mut text)?;
    if text.len() > 65536 {
        return Err("synthetic clipboard fixture exceeds 64 KiB".into());
    }
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text)?;
    println!("clipboard fixture ready");
    std::thread::sleep(std::time::Duration::from_secs(60));
    Ok(())
}
