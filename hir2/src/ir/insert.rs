/// Represents the placement of inserted items relative to a [ProgramPoint]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Insert {
    /// New items will be inserted before the current program point
    Before,
    /// New items will be inserted after the current program point
    After,
}
