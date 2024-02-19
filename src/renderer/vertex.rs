pub struct ItfVertBuffer {
    buffer: Subbuffer<ItfVertInfo>,
    ranges: HashMap<BinID, Range<usize>>,
}

pub struct TransferRange {
    src: Range<usize>,
    dst: Range<usize>,
}

pub struct StagingBuffer {
    buffer: Subbuffer<ItfVertInfo>,
    ranges: HashMap<BinID, TransferRange>,
}