pub(crate) const READ_DEFAULT_OFFSET: u32 = 1;
pub(crate) const READ_DEFAULT_LIMIT: u32 = 2000;

pub(crate) fn normalize_read_offset(offset: u32) -> u32 {
    offset.max(READ_DEFAULT_OFFSET)
}

pub(crate) fn normalize_read_limit(limit: u32) -> u32 {
    if limit == 0 {
        READ_DEFAULT_LIMIT
    } else {
        limit
    }
}
