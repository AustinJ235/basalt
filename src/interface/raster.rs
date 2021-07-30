use crate::interface::bin::Bin;
use crate::atlas::Coords;
use crate::image_view::BstImageView;
use crate::interface::interface::ItfVertInfo;
use crate::Basalt;
use std::sync::{Arc, Weak};
use std::collections::BTreeMap;
use std::time::Instant;

type BinID = u64;
type ZIndex = i16;

pub struct BstRaster {

}

struct Core {
    bst: Arc<Basalt>,
    bins: BTreeMap<BinID, BinData>,
    layers: BTreeMap<ZIndex, Layer>,
}

struct BinData {
    bin: Weak<Bin>,
    inst: Instant,
    scale: f32,
    extent: [u32; 2],
    in_layers: Vec<ZIndex>,
}

struct Layer {
    vertex: BTreeMap<BinID, Vec<VertexData>>,
    composed: Option<LayerComposed>,
}

struct VertexData {
    img: VertexImage,
    data: Vec<ItfVertInfo>,
}

enum VertexImage {
    None,
    Atlas(Coords),
    Custom(BstImageView, Coords),
}

struct LayerComposed {
    vertex: Vec<(VertexImageComposed, Vec<ItfVertInfo>)>,
}

struct VertexImageComposed {
    coords: Coords,
    image: BstImageView,
}

impl Core {
    fn new(bst: Arc<Basalt>) -> Self {
        Self {
            bst,
            bins: BTreeMap::new(),
            layers: BTreeMap::new(),
        }
    }

    fn update_bins(&mut self, scale: f32, extent: [u32; 2]) -> bool {
        #[derive(PartialEq, Eq)]
        enum BinStatus {
            Exists,
            Create,
            Remove,
        }

        #[derive(PartialEq, Eq)]
        enum UpdateStatus {   
            Current,
            Wanted,
            Required,
        }

        let contained_ids: Vec<BinID> = self.bins.keys().cloned().collect();
        let mut all_bins: BTreeMap<BinID, Arc<Bin>> = BTreeMap::new();

        for bin in self.bst.interface_ref().bins() {
            all_bins.insert(bin.id(), bin);
        }

        let mut bin_state: BTreeMap<BinID, (BinStatus, UpdateStatus)> = BTreeMap::new();

        for id in contained_ids {
            if all_bins.contains_key(&id) {
                let bin_data = self.bins.get(&id).unwrap();
                let bin = all_bins.get(&id).unwrap();

                let update_status = if bin.wants_update() {
                    UpdateStatus::Wanted
                } else if bin_data.extent != extent || bin_data.scale != scale {
                    UpdateStatus::Required
                } else if bin_data.inst < bin.last_update() {
                    UpdateStatus::Required
                } else {
                    UpdateStatus::Current
                };

                bin_state.insert(id, (BinStatus::Exists, update_status));
            } else {
                bin_state.insert(id, (BinStatus::Remove, UpdateStatus::Current));
            }
        }

        for (id, bin) in all_bins.iter() {
            if !bin_state.contains_key(id) {
                let post_up = bin.post_update();

                let update_status = if bin.wants_update() {
                    UpdateStatus::Wanted
                } else if post_up.extent != extent || post_up.scale != scale {
                    UpdateStatus::Required
                } else {
                    UpdateStatus::Current
                };

                bin_state.insert(*id, (BinStatus::Create, update_status));
            }
        }

        for (id, (status, update)) in bin_state {
            if status == BinStatus::Remove || update == UpdateStatus::Wanted || update == UpdateStatus::Required {

            }


        }

        false
    }
}