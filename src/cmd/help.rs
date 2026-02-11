use anyhow::Result;

pub fn run() -> Result<()> {
    println!("pikpaktui - A TUI and CLI client for PikPak cloud storage");
    println!();
    println!("Usage: pikpaktui [command] [args...]");
    println!();
    println!("  (no command)    Launch interactive TUI");
    println!("  ls [-l] [path]  List files (colored grid by default, long with -l)");
    println!("  mv <src> <dst>  Move a file or folder");
    println!("  cp <src> <dst>  Copy a file or folder");
    println!("  rename <path> <new_name>  Rename a file or folder");
    println!("  rm <path>       Remove a file or folder (to trash)");
    println!("  mkdir <parent> <name>     Create a new folder");
    println!("  download <path> [local]   Download a file");
    println!("  upload <local> [remote]   Upload a local file");
    println!("  share <path> [-o file]    Share file(s) as PikPak links");
    println!("  quota           Show storage quota");
    println!("  help, --help    Show this help message");
    Ok(())
}
