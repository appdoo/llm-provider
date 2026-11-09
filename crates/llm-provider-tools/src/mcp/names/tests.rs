use super::*;

fn remotes(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_valid_name_stays_as_it_is() {
    let names = model_names("server", None, &remotes(&["read_file", "add-2"]));

    assert_eq!(names.expect("valid"), ["read_file", "add-2"]);
}

#[test]
fn every_refused_character_becomes_an_underscore() {
    let names = model_names("server", None, &remotes(&["fs.read", "a b/c"]));

    assert_eq!(names.expect("valid"), ["fs_read", "a_b_c"]);
}

#[test]
fn the_prefix_is_part_of_the_sanitised_name() {
    let names = model_names("server", Some("my server"), &remotes(&["fs.read"]));

    assert_eq!(names.expect("valid"), ["my_server_fs_read"]);
}

#[test]
fn a_name_of_64_characters_is_accepted() {
    let tool = "t".repeat(64);

    let names = model_names("server", None, &remotes(&[&tool]));

    assert_eq!(names.expect("valid"), [tool]);
}

#[test]
fn a_prefixed_name_over_64_characters_is_an_error() {
    let prefix = "p".repeat(60);

    let refused = model_names("server", Some(&prefix), &remotes(&["read"]));

    match refused {
        Err(Error::NameTooLong { server, tool, name }) => {
            assert_eq!(server, "server");
            assert_eq!(tool, "read");
            assert_eq!(name, format!("{prefix}_read"));
        }
        other => panic!("expected a name error, got {other:?}"),
    }
}

#[test]
fn two_names_that_sanitise_alike_are_an_error() {
    let refused = model_names("server", None, &remotes(&["echo", "fs.read", "fs_read"]));

    match refused {
        Err(Error::NameCollision {
            server,
            tools,
            name,
        }) => {
            assert_eq!(server, "server");
            assert_eq!(tools, ["fs.read", "fs_read"]);
            assert_eq!(name, "fs_read");
        }
        other => panic!("expected a collision, got {other:?}"),
    }
}
