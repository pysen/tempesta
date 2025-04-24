// use crate::add;
use assert_cmd::Command;

#[test]
fn tempesta_init() {
  Command::cargo_bin("tempesta")
    .unwrap()
    .arg("init")
    .write_stdin("~/.bookmark-store-test\nno\n")
    .assert()
    .success()
    //TODO: home_dir variable in stdout?
    .stdout("Where do you want to store the bookmarks? [~/.bookmark-store]: Do you want to use Git for tracking bookmarks? (Y/n): Tempesta initialized successfully: /home/art/.config/tempesta/tempesta.toml\n");
  //TODO: assert file is created and looks as expected
}

#[test]
fn tempesta_add_move_remove() {
  // add
  Command::cargo_bin("tempesta")
    .unwrap()
    .args(["add", "test", "https://test.local", "test"])
    .assert()
    .success()
    //TODO: home_dir variable in stdout?
    .stdout("Bookmark file stored at /home/art/.bookmark-store-test/test.toml\nBookmark added successfully as test\n");
  // move
  Command::cargo_bin("tempesta")
    .unwrap()
    .args(["move", "test", "move/test"])
    .assert()
    .success()
    .stdout("Bookmark moved successfully from test to move/test\n");
  // remove
  Command::cargo_bin("tempesta")
    .unwrap()
    .args(["remove", "move/test"])
    .assert()
    .success()
    .stdout("Bookmark removed successfully as move/test\n");
  // TODO: cleanup bookmark-store-test(?)
  // TODO: cleanup tempesta.toml(?)
}
