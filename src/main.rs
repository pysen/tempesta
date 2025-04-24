#[cfg(test)]
mod tests;

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Deserialize)]
struct Bookmark {
  url: String,
  tags: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Config {
  git: bool,
  remote: Option<String>,
  dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
  Bash,
  Zsh,
  Fish,
}

trait PanicOnError<T> {
  fn panic_on_error(self, msg: &str) -> T;
}

impl Shell {
  pub fn to_str(&self) -> &'static str {
    match self {
      Shell::Bash => "bash",
      Shell::Zsh => "zsh",
      Shell::Fish => "fish",
    }
  }
  pub fn from_shell_str(s: &str) -> Option<Self> {
    match s {
      "bash" => Some(Shell::Bash),
      "zsh" => Some(Shell::Zsh),
      "fish" => Some(Shell::Fish),
      _ => None,
    }
  }
  pub fn filename(&self) -> String {
    match self {
      Shell::Bash => "tempesta-completion.bash.sh".to_string(),
      Shell::Zsh => "tempesta-completion.zsh.sh".to_string(),
      Shell::Fish => "tempesta-completion.fish.sh".to_string(),
    }
  }
  pub fn completion_script(&self) -> &'static str {
    match self {
      Shell::Bash => BASH_COMPLETION,
      Shell::Zsh => ZSH_COMPLETION,
      Shell::Fish => FISH_COMPLETION,
    }
  }
}

// Implement for Result<T, E>
impl<T, E: std::fmt::Display> PanicOnError<T> for Result<T, E> {
  fn panic_on_error(self, msg: &str) -> T {
    self.unwrap_or_else(|e| panic!("{}: {}", msg, e))
  }
}

// Implement for Option<T>
impl<T> PanicOnError<T> for Option<T> {
  fn panic_on_error(self, msg: &str) -> T {
    self.unwrap_or_else(|| panic!("{}", msg))
  }
}

fn main() {
  let args: Vec<String> = env::args().collect();
  if args.len() < 2 {
    eprintln!("Usage: tempesta <command> [options]");
    std::process::exit(1);
  }
  let command = &args[1];
  match command.as_str() {
    "add" | "a" => add(args),
    "completion" => completion(args),
    "config" | "c" => config(),
    "edit" | "e" => edit(args),
    "init" | "i" => init(),
    "list" | "l" => list(args),
    "move" | "m" => r#move(args),
    "open" | "o" => open(args),
    "remove" | "r" => remove(args),
    "update" | "u" => update(args),
    "--version" | "-v" => print_version(),
    _ => {
      eprintln!("Unknown command: {}", command);
      eprintln!(
        "Available commands: [a]dd, [e]dit, [l]ist, [m]ove, [o]pen, [r]emove, [u]pdate"
      );
      std::process::exit(1);
    }
  }
  std::process::exit(0);
}

fn config() {
  let config = load_config();
  println!("Git enabled: {}", config.git);
  if let Some(remote) = &config.remote {
    println!("Remote: {}", remote);
  } else {
    println!("Remote: None");
  }
  println!("Bookmark store directory: {}", config.dir);
}

fn completion(args: Vec<String>) {
  let detected_shell = detect_shell()
    .as_deref()
    .and_then(Shell::from_shell_str)
    .unwrap_or(Shell::Bash);
  let mut selected_shell = detected_shell;
  if args.len() > 2 {
    if let Some(shell) = Shell::from_shell_str(&args[2]) {
      selected_shell = shell;
    }
  }
  let script = selected_shell.completion_script();
  println!("{}", script)
}

fn print_version() {
  println!("Tempesta version: {}", env!("CARGO_PKG_VERSION"));
}

fn init() {
  let storage_path = prompt_valid_bookmark_store_path();

  print!("Do you want to use Git for tracking bookmarks? (Y/n): ");
  io::stdout()
    .flush()
    .panic_on_error("Failed to flush stdout");
  let mut input = String::new();
  io::stdin()
    .read_line(&mut input)
    .panic_on_error("Failed to read input");
  let use_git = !matches!(input.trim().to_lowercase().as_str(), "n" | "no");
  let config = Config {
    git: use_git,
    remote: None,
    dir: storage_path,
  };
  save_config(&config);
  if use_git {
    handle_git(&config);
  }
  let config_file_path = get_config_file_path();
  println!(
    "Tempesta initialized successfully: {}",
    config_file_path.display()
  );
}

fn prompt_valid_bookmark_store_path() -> String {
  loop {
    let mut storage_path = String::new();
    print!("Where do you want to store the bookmarks? [~/.bookmark-store]: ");
    io::stdout().flush().unwrap();
    io::stdin()
      .read_line(&mut storage_path)
      .expect("Failed to read input");
    let storage_path = storage_path.trim();

    // Default path if the user input is empty
    let storage_path = if storage_path.is_empty() {
      let home_dir = dirs::home_dir().expect("Could not find home directory");
      let mut default_dir = home_dir;
      default_dir.push(".bookmark-store");
      default_dir.to_string_lossy().into_owned()
    } else {
      let expanded = expand_tilde(storage_path);
      let path = Path::new(&expanded);

      // Check if the path is valid (absolute or has a parent directory)
      if !(path.is_absolute() || path.parent().is_some()) {
        println!("Invalid path format. Please enter a valid absolute or relative path.");
        continue;
      }

      // Check if the path has write permission
      if !check_write_permission(path) {
        println!(
          "No write permission for the specified path: {}",
          expanded.to_string_lossy()
        );
        continue;
      }

      expanded.to_string_lossy().into_owned()
    };

    return storage_path;
  }
}

fn check_write_permission(path: &Path) -> bool {
  // Get the parent directory or use the path itself if it's already a directory
  let parent = if path.is_dir() {
    path
  } else {
    path.parent().unwrap_or(path)
  };

  let test_file = parent.join(".permission_check");
  match File::create(&test_file) {
    Ok(_) => {
      let _ = fs::remove_file(&test_file);
      true
    }
    Err(_) => false,
  }
}

fn add(args: Vec<String>) {
  if args.len() < 4 {
    eprintln!("Usage: tempesta add <path> <url> [tags...]");
    std::process::exit(1);
  }
  let relative_path = &args[2];
  validate_path(relative_path);
  let toml_file_path = get_bookmark_file_path(relative_path);
  if toml_file_path.exists() {
    print!(
      "Bookmark already exists at {}. Overwrite? (y/N): ",
      toml_file_path.display()
    );
    io::stdout()
      .flush()
      .panic_on_error("Failed to flush stdout");

    let mut input = String::new();
    io::stdin()
      .read_line(&mut input)
      .panic_on_error("Failed to read input");

    let input = input.trim().to_lowercase();
    if input.is_empty() || ["n", "no"].contains(&input.as_str()) {
      println!("Operation cancelled.");
      return;
    }
    if !["y", "yes"].contains(&input.as_str()) {
      println!("Invalid input. Operation cancelled.");
      return;
    }
    println!("Overwriting file...");
  }
  let url = &args[3];
  validate_url(url);
  let tags = &args[4..].to_vec();
  store_bookmark(&toml_file_path, url, tags);
  let comment = format!("Add bookmark {}", &relative_path);
  git_commit(&comment);
  println!("Bookmark added successfully as {}", &relative_path);
}

fn r#move(args: Vec<String>) {
  if args.len() < 4 {
    eprintln!("Usage: tempesta move <path-from> <path-to>");
    std::process::exit(1);
  }
  let relative_path_from = &args[2];
  validate_path(relative_path_from);
  let relative_path_to = &args[3];
  validate_path(relative_path_to);
  let toml_from_file_path = get_bookmark_file_path(relative_path_from);
  if !toml_from_file_path.exists() {
    eprintln!("Path {:?} do not exists", &toml_from_file_path.to_str());
    std::process::exit(1);
  }
  let toml_to_file_path = get_bookmark_file_path(relative_path_to);
  if toml_to_file_path.exists() && !prompt_for_overwrite(&toml_to_file_path) {
    println!("Move operation aborted.");
    std::process::exit(0);
  }

  if let Some(parent) = toml_to_file_path.parent() {
    if !parent.exists() {
      fs::create_dir_all(parent)
        .panic_on_error("Failed to create destination directory")
    }
  }

  fs::rename(&toml_from_file_path, &toml_to_file_path)
    .panic_on_error("Failed to move bookmark file");

  // After successful move, cleanup empty parent directories
  if let Some(parent) = toml_from_file_path.parent() {
    cleanup_empty_parents(parent)
      .panic_on_error("Warning: Failed to clean up empty directories")
  }

  let comment = format!(
    "Move bookmark from {} to {}",
    &relative_path_from, &relative_path_to
  );
  git_commit(&comment);

  println!(
    "Bookmark moved successfully from {} to {}",
    &relative_path_from, &relative_path_to
  );
}

fn update(args: Vec<String>) {
  if args.len() < 4 {
    eprintln!("Usage: tempesta update <path> <url> [tags...]");
    std::process::exit(1);
  }
  let relative_path = &args[2];
  validate_path(relative_path);
  let toml_file_path = get_bookmark_file_path(relative_path);
  if !toml_file_path.exists() {
    eprintln!("Path {:?} do not exists", &toml_file_path.to_str());
    std::process::exit(1);
  }
  let url = &args[3];
  validate_url(url);
  let tags = &args[4..].to_vec();
  store_bookmark(&toml_file_path, url, tags);
  let comment = format!("Update bookmark {}", &relative_path);
  git_commit(&comment);

  println!("Bookmark updated successfully as {}", &relative_path);
}

fn list(args: Vec<String>) {
  let bookmarks = if args.len() > 2 {
    get_toml_bookmark_files(Some(args[2].clone()))
  } else {
    get_toml_bookmark_files(None)
  };
  if bookmarks.is_empty() {
    eprintln!("No bookmarks found.");
    return;
  }
  let mut divisor = " :: ".to_string();

  // Parse args to find --divisor value
  let mut args_iter = args.iter();
  while let Some(arg) = args_iter.next() {
    if arg == "--divisor" {
      if let Some(value) = args_iter.next() {
        divisor = value.to_string();
      }
    } else if arg.starts_with("--divisor=") {
      if let Some(value) = arg.split_once('=').map(|x| x.1) {
        divisor = value.to_string();
      }
    }
  }
  let formatted = bookmarks.iter().map(|path| {
    let mut current_path = get_bookmark_store_dir_path();
    current_path.push(PathBuf::from(path));
    let full_path = format!("{}.toml", &current_path.display());
    let url =
      extract_url_from_toml(&full_path).unwrap_or_else(|_| "N/A".to_string());
    format!("{}{}{}", path, divisor, url)
  });
  for line in formatted {
    println!("{}", line);
  }
}

fn open(args: Vec<String>) {
  let relative_path = if args.len() < 3 {
    // No path provided, try to invoke finder
    eprintln!("Usage: tempesta open <path>");
    std::process::exit(1);
  } else {
    args[2].clone()
  };
  validate_path(&relative_path);
  let url = get_url(&relative_path);
  validate_url(&url);
  webbrowser::open(&url).panic_on_error("Failed to open browser");
}

// Example function to extract the URL from the TOML file
fn extract_url_from_toml(
  path: &str,
) -> Result<String, Box<dyn std::error::Error>> {
  let content = fs::read_to_string(path)?;
  let toml: toml::Value = toml::from_str(&content)?;
  toml
    .get("url")
    .and_then(|v| v.as_str())
    .map(String::from)
    .ok_or_else(|| "Missing or invalid `url`".into())
}

fn get_toml_bookmark_files(sub_path: Option<String>) -> Vec<String> {
  let root_dir = get_bookmark_store_dir_path();
  let search_dir = match &sub_path {
    Some(sub) => {
      let mut d = root_dir.clone();
      d.push(sub);
      d
    }
    None => root_dir.clone(),
  };
  let mut bookmarks = Vec::new();
  fn visit_dir(dir: &PathBuf, root_dir: &PathBuf, bookmarks: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
      for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
          visit_dir(&path, root_dir, bookmarks); // recurse
        } else if path.is_file()
          && path.extension().is_some_and(|ext| ext == "toml")
        {
          if let Ok(relative_path) = path.strip_prefix(root_dir) {
            if let Some(relative_str) = relative_path.to_str() {
              let without_extension = relative_str.trim_end_matches(".toml");
              bookmarks.push(without_extension.to_string());
            }
          }
        }
      }
    }
  }
  visit_dir(&search_dir, &root_dir, &mut bookmarks);
  if bookmarks.is_empty() {
    eprintln!("No .toml files found in {:?}", search_dir);
  }
  bookmarks
}

fn remove(args: Vec<String>) {
  if args.len() < 3 {
    eprintln!("Usage: tempesta remove <path>");
    std::process::exit(1);
  }
  let relative_path = &args[2];
  let toml_file_path = get_bookmark_file_path(relative_path);
  if toml_file_path.exists() {
    fs::remove_file(&toml_file_path).panic_on_error("Failed to remove file");
    println!("Bookmark removed successfully as {}", &relative_path);
    let mut parent_dir = toml_file_path.parent();
    while let Some(dir) = parent_dir {
      if fs::remove_dir(dir).is_ok() {
        parent_dir = dir.parent();
      } else {
        break;
      }
    }
    let comment = format!("Remove bookmark {}", &relative_path);
    git_commit(&comment);
    return;
  }
  let mut given_path = get_bookmark_store_dir_path();
  given_path.push(relative_path);
  if !given_path.is_dir() {
    eprintln!("Bookmark not found: {}", toml_file_path.display());
    return;
  }
  let prompt_message = format!(
    "Bookmark not found as a file, but '{}' is a directory. \
    Do you want to delete it and all its bookmarks? [Y/n] ",
    relative_path
  );
  print!("{}", prompt_message);
  let mut input = String::new();
  io::stdout()
    .flush()
    .panic_on_error("Cannot flush prompt delete dir");
  io::stdin()
    .read_line(&mut input)
    .panic_on_error("Cannot read input delete dir");
  let input = input.trim().to_lowercase();
  if input.is_empty() || input == "y" || input == "yes" {
    fs::remove_dir_all(&given_path)
      .panic_on_error("Failed to remove directory");
    let comment =
      format!("Removed directory {} and all bookmarks", relative_path);
    git_commit(&comment);
    println!("Directory and all bookmarks removed: {}", relative_path);
    return;
  }
  println!("Operation canceled.");
}

fn edit(args: Vec<String>) {
  if args.len() < 3 {
    eprintln!("Usage: tempesta edit <path>");
    std::process::exit(1);
  }
  let relative_path = &args[2];
  validate_path(relative_path);
  let toml_file_path = get_bookmark_file_path(relative_path);
  if !toml_file_path.exists() {
    eprintln!("Bookmark file does not exist: {}", toml_file_path.display());
    std::process::exit(1);
  }
  // Get preferred editor from $EDITOR, or default to nano
  let editor = env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
  // Store last modified timestamp before editing
  let metadata_before = fs::metadata(&toml_file_path)
    .and_then(|m| m.modified())
    .ok();
  // Open the file in the preferred editor (blocking)
  let status = Command::new(&editor)
    .arg(&toml_file_path)
    .status()
    .panic_on_error("Failed to open editor");
  if !status.success() {
    eprintln!("Failed to edit bookmark file.");
    return;
  }
  // Check if the file was modified
  let metadata_after = fs::metadata(&toml_file_path)
    .and_then(|m| m.modified())
    .ok();
  if metadata_before != metadata_after {
    let comment = format!("Edit bookmark {}", &toml_file_path.display());
    git_commit(&comment);
    println!("Bookmark edited successfully as {}", &relative_path);
  } else {
    println!("No changes made.");
  }
}

fn get_config_file_path() -> PathBuf {
  let home_dir =
    dirs::home_dir().panic_on_error("Could not find home directory");
  let mut config_path = home_dir;
  config_path.push(".config/tempesta");
  fs::create_dir_all(&config_path)
    .panic_on_error("Failed to create config directory");
  config_path.push("tempesta.toml");
  config_path
}

fn load_config() -> Config {
  let config_file_path = get_config_file_path();
  let content = fs::read_to_string(&config_file_path)
    .panic_on_error("Cannot read config file");
  let config: Config =
    toml::from_str(&content).panic_on_error("Cannot read toml config file");
  config
}

fn save_config(config: &Config) {
  let config_file_path = get_config_file_path();
  let content =
    toml::to_string(config).panic_on_error("Cannot write toml config file");
  fs::write(config_file_path, content)
    .panic_on_error("Cannot write config file");
}

fn handle_git(previous_config: &Config) {
  let git_remote = prompt_remote_url();
  let bookmark_store_dir_path = get_bookmark_store_dir_path();

  run_command(
    "git",
    &["init"],
    &bookmark_store_dir_path,
    "Failed to initialize Git repository",
  );
  println!(
    "Git repository initialized at {}",
    bookmark_store_dir_path.display()
  );

  if let Some(remote) = &git_remote {
    let branch_name = prompt_branch_name();
    run_command(
      "git",
      &["remote", "add", "origin", remote],
      &bookmark_store_dir_path,
      "Failed to add remote repository",
    );
    println!("Git remote repository set to {}", remote);
    run_command(
      "git",
      &["pull", "origin", &branch_name],
      &bookmark_store_dir_path,
      "Failed to pull from origin",
    );
  }

  let config = Config {
    git: true,
    remote: git_remote,
    dir: previous_config.dir.clone(),
  };
  save_config(&config);
}

fn prompt_remote_url() -> Option<String> {
  print!("Enter the remote repository URI (leave empty for no remote): ");
  io::stdout()
    .flush()
    .panic_on_error("Failed to flush stdout");

  let mut input = String::new();
  io::stdin()
    .read_line(&mut input)
    .panic_on_error("Failed to read input");
  let trimmed = input.trim();

  if trimmed.is_empty() {
    None
  } else {
    Some(trimmed.to_string())
  }
}

fn prompt_branch_name() -> String {
  print!("Enter the branch name to pull from [master]: ");
  io::stdout()
    .flush()
    .panic_on_error("Failed to flush stdout");
  let mut input = String::new();
  io::stdin()
    .read_line(&mut input)
    .panic_on_error("Failed to read input");
  let trimmed = input.trim();
  if trimmed.is_empty() {
    "master".to_string() // Default to "master" if no input is given
  } else {
    trimmed.to_string()
  }
}

fn run_command(
  cmd: &str,
  args: &[&str],
  dir: &std::path::Path,
  error_message: &str,
) {
  Command::new(cmd)
    .args(args)
    .current_dir(dir)
    .output()
    .panic_on_error(error_message);
}

fn get_bookmark_store_dir_path() -> PathBuf {
  let config = load_config();
  let expanded_dir = expand_tilde(&config.dir);
  fs::create_dir_all(&expanded_dir)
    .panic_on_error("Failed to create bookmark store");
  expanded_dir
}

fn expand_tilde(path: &str) -> PathBuf {
  if path == "~" {
    return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
  } else if path.starts_with("~/") {
    if let Some(home) = dirs::home_dir() {
      return home.join(path.trim_start_matches("~/"));
    }
  }
  PathBuf::from(path)
}

fn validate_path(relative_path: &str) {
  let re =
    Regex::new(r"^[a-zåäöA-ZÅÄÖ0-9_/.-]+$").panic_on_error("Invalid path");
  if !re.is_match(relative_path) {
    panic!("Invalid path. Please avoid spaces and special characters.");
  }
}

fn validate_url(url: &str) {
  let re = Regex::new(r"^(https?|ftp)://[^\s/$.?#].[^\s]*$")
    .panic_on_error("Invalid url format");
  if !re.is_match(url) {
    panic!(
      "Invalid URL. Please use a proper format (e.g., https://example.com)."
    );
  }
}

fn get_bookmark_file_path(relative_path: &String) -> PathBuf {
  let mut bookmark_store_dir_path = get_bookmark_store_dir_path();
  let relative_path_buf = PathBuf::from(relative_path);
  let file_name = relative_path_buf
    .file_name()
    .panic_on_error("Invalid path provided")
    .to_string_lossy()
    .to_string()
    + ".toml";
  let parent_path = relative_path_buf
    .parent()
    .map(|p| p.to_path_buf())
    .unwrap_or_else(|| PathBuf::from("."));
  bookmark_store_dir_path.push(parent_path);
  fs::create_dir_all(&bookmark_store_dir_path)
    .panic_on_error("Failed to create directory");
  bookmark_store_dir_path.push(file_name);
  bookmark_store_dir_path
}

fn store_bookmark(toml_file_path: &PathBuf, url: &str, tags: &[String]) {
  let bookmark = Bookmark {
    url: url.to_owned(),
    tags: tags.to_owned(),
  };
  let toml_content =
    toml::to_string(&bookmark).panic_on_error("Failed to serialize bookmark");
  fs::write(toml_file_path, toml_content)
    .panic_on_error("Failed to write bookmark file");
  println!("Bookmark file stored at {}", toml_file_path.display())
}

fn get_url(relative_path: &String) -> String {
  let toml_file_path = get_bookmark_file_path(relative_path);
  let toml_content =
    fs::read_to_string(toml_file_path).panic_on_error("Failed to read TOML");
  let bookmark: Bookmark = toml::from_str(&toml_content)
    .panic_on_error("Failed to parse TOML content");
  bookmark.url
}

fn push_to_origin() {
  let config = load_config();
  if config.remote.is_none() {
    return;
  }
  println!("Pushing changes to remote origin...");
  git_command(&["push", "-u", "--all"], "Cannot push to origin");
}

fn git_commit(comment: &str) {
  git_command(&["add", "-A"], "Failed to add file to git stage");
  git_command(&["commit", "-m", comment], "Failed to commit to git");
  push_to_origin();
}

fn git_command(args: &[&str], error_message: &str) {
  let config = load_config();
  if !config.git {
    return;
  }
  let bookmark_store_dir_path = get_bookmark_store_dir_path();
  run_command("git", args, &bookmark_store_dir_path, error_message);
}

const BASH_COMPLETION: &str =
  include_str!("completions/tempesta-completion.bash.sh");
const ZSH_COMPLETION: &str =
  include_str!("completions/tempesta-completion.zsh.sh");
const FISH_COMPLETION: &str =
  include_str!("completions/tempesta-completion.fish.sh");

fn detect_shell() -> Option<String> {
  env::var("SHELL").ok().and_then(|shell_path| {
    shell_path.split('/').next_back().map(|s| s.to_string())
  })
}

/// Recursively removes parent directories if they are empty.
/// Stops once a non-empty directory is found or the root is reached.
fn cleanup_empty_parents(starting_dir: &Path) -> std::io::Result<()> {
  let mut current = starting_dir.to_path_buf();
  loop {
    // If directory does not exist (already removed), we just break.
    if !current.exists() {
      break;
    }
    // Check if the directory is empty.
    let is_empty = fs::read_dir(&current)?.next().is_none();
    if is_empty {
      // Try to remove the directory.
      fs::remove_dir(&current)?;
      // Move to parent directory for the next check.
      if !current.pop() {
        break; // Reached the root.
      }
    } else {
      break; // Stop if we hit a non-empty directory.
    }
  }
  Ok(())
}

fn prompt_for_overwrite(destination: &Path) -> bool {
  print!(
    "A bookmark already exists at {}. Overwrite? [Y/n]: ",
    destination.display()
  );
  io::stdout().flush().expect("Failed to flush stdout");

  let mut input = String::new();
  io::stdin()
    .read_line(&mut input)
    .expect("Failed to read user input");

  let trimmed = input.trim().to_lowercase();

  match trimmed.as_str() {
    "" | "y" | "yes" => true, // Default to yes on Enter
    "n" | "no" => false,
    _ => {
      println!("Invalid input, assuming 'no'.");
      false
    }
  }
}
