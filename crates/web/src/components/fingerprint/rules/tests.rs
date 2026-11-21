use super::*;

#[test]
fn a_fingerprint_is_drawn_in_its_five_groups_of_four() {
    assert_eq!(
        groups("7K3Q-M9XD-2RTV-8HFA-QW5N"),
        ["7K3Q", "M9XD", "2RTV", "8HFA", "QW5N"]
    );
}

#[test]
fn every_character_is_drawn_so_none_is_shown_cut_short() {
    assert_eq!(
        groups("7K3QM9XD2RTV8HFAQW5N"),
        ["7K3Q", "M9XD", "2RTV", "8HFA", "QW5N"]
    );
    assert_eq!(groups("7K3Q-M9XD-2R"), ["7K3Q", "M9XD", "2R"]);
    assert_eq!(groups(""), Vec::<String>::new());
}
