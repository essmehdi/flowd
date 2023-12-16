use shellexpand::tilde;

pub fn expand(path: &str) -> String {
    String::from(tilde(path))
}
