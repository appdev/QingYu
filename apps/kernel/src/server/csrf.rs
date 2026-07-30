#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestIntent {
    ReadOnly,
    StateChanging,
}
