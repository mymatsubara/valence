use bevy_ecs::prelude::*;
use bevy_ecs::{query::Changed, system::Query};
use valence_protocol::packets::s2c::set_equipment::SetEquipment;
use valence_protocol::VarInt;
use valence_protocol::{packets::s2c::set_equipment::EquipmentEntry, ItemStack};

use crate::prelude::*;
use crate::view::ChunkPos;

/// Number of equipment slots
pub const EQUIPMENT_SLOTS: usize = 6;

#[derive(Copy, Clone)]
pub enum EquipmentSlot {
    MainHand,
    OffHand,
    Boots,
    Leggings,
    Chestplate,
    Helmet,
}

/// ECS component to be added for entities with equipments.
///
/// Equipment updates managed by [update_equipment].
#[derive(Component, Default, PartialEq, Debug)]
pub struct Equipments {
    equipments: [Option<Box<EquipmentEntry>>; EQUIPMENT_SLOTS],
    /// Bit set with the modified equipment slots
    modified_slots: u8,
}

impl Equipments {
    pub fn new() -> Equipments {
        Equipments::default()
    }

    /// Set an equipment slot with an item stack
    pub fn set(&mut self, item: ItemStack, slot: EquipmentSlot) {
        let slot_idx: usize = slot.into();
        self.equipments[slot_idx] = Some(Box::new(EquipmentEntry {
            slot: slot_idx as i8,
            item: Some(item),
        }));

        self.set_modified_slot(slot);
    }

    /// Remove all equipments
    pub fn clear(&mut self) {
        for slot in self.equipments.iter_mut() {
            if let Some(equip) = slot {
                self.modified_slots |= 1 << equip.slot as u8;
                *slot = None;
            }
        }
    }

    /// Remove an equipment from a slot and return it if present
    pub fn remove(&mut self, slot: EquipmentSlot) -> Option<EquipmentEntry> {
        let slot_idx: usize = slot.into();

        if let Some(equipment) = (&mut self.equipments[slot_idx]).take() {
            self.set_modified_slot(slot);
            Some(*equipment)
        } else {
            None
        }
    }

    pub fn get(&self, slot: EquipmentSlot) -> Option<Box<EquipmentEntry>> {
        let slot_idx: usize = slot.into();
        if let Some(equipment) = self.equipments[slot_idx] {
            Some(equipment)
        } else {
            None
        }
    }

    pub fn equiped(&self) -> impl Iterator<Item = &Box<EquipmentEntry>> + '_ {
        self.equipments.iter().filter_map(|equip| equip.as_ref())
    }

    pub fn is_empty(&self) -> bool {
        self.equipments.is_empty()
    }

    fn has_modified_slots(&self) -> bool {
        self.modified_slots != 0
    }

    fn iter_modified_equipments(&self) -> impl Iterator<Item = EquipmentEntry> + '_ {
        self.iter_modified_slots().map(|slot| {
            self.get(slot)
                .map(|equip| *equip)
                .unwrap_or_else(|| EquipmentEntry {
                    slot: slot.into(),
                    item: None,
                })
        })
    }

    fn iter_modified_slots(&self) -> impl Iterator<Item = EquipmentSlot> {
        let modified_slots = self.modified_slots;

        (0..EQUIPMENT_SLOTS).filter_map(move |slot| {
            if modified_slots & (1 << slot) != 0 {
                Some(EquipmentSlot::try_from(slot).unwrap())
            } else {
                None
            }
        })
    }

    fn set_modified_slot(&mut self, slot: EquipmentSlot) {
        let shifts: usize = slot.into();
        self.modified_slots |= 1 << shifts;
    }

    fn clear_modified_slot(&mut self) {
        self.modified_slots = 0;
    }
}

impl TryFrom<u8> for EquipmentSlot {
    type Error = &'static str;

    /// Convert from `id` according to https://wiki.vg/Protocol#Set_Equipment
    fn try_from(id: u8) -> Result<Self, Self::Error> {
        let slot = match id {
            0 => EquipmentSlot::MainHand,
            1 => EquipmentSlot::OffHand,
            2 => EquipmentSlot::Boots,
            3 => EquipmentSlot::Leggings,
            4 => EquipmentSlot::Chestplate,
            5 => EquipmentSlot::Helmet,
            _ => return Err("Invalid value"),
        };

        Ok(slot)
    }
}

impl TryFrom<usize> for EquipmentSlot {
    type Error = &'static str;

    fn try_from(id: usize) -> Result<Self, Self::Error> {
        EquipmentSlot::try_from(id as u8)
    }
}

impl TryFrom<i8> for EquipmentSlot {
    type Error = &'static str;

    fn try_from(id: i8) -> Result<Self, Self::Error> {
        EquipmentSlot::try_from(id as u8)
    }
}

impl From<EquipmentSlot> for u8 {
    /// Convert to `id` according to https://wiki.vg/Protocol#Set_Equipment
    fn from(slot: EquipmentSlot) -> Self {
        match slot {
            EquipmentSlot::MainHand => 0,
            EquipmentSlot::OffHand => 1,
            EquipmentSlot::Boots => 2,
            EquipmentSlot::Leggings => 3,
            EquipmentSlot::Chestplate => 4,
            EquipmentSlot::Helmet => 5,
        }
    }
}

impl From<EquipmentSlot> for i8 {
    fn from(slot: EquipmentSlot) -> Self {
        EquipmentSlot::from(slot) as Self
    }
}

impl From<EquipmentSlot> for usize {
    fn from(slot: EquipmentSlot) -> Self {
        EquipmentSlot::from(slot) as Self
    }
}

/// When a [Equipments] component is changed, send [SetEquipment] packet to all clients
/// that have the updated entity in their view distance.
///
/// NOTE: [SetEquipment] packet only have cosmetic effect, which means it does not affect armor resistance or damage.
pub fn update_equipment(
    mut equiped_entities: Query<(Entity, &McEntity, &mut Equipments), Changed<Equipments>>,
    mut clients: Query<(Entity, &mut Client)>,
) {
    for (equiped_entity, equiped_mc_entity, mut equips) in &mut equiped_entities {
        if !equips.has_modified_slots() {
            continue;
        }

        let equiped_instance = equiped_mc_entity.instance();
        let equiped_chunk_pos = ChunkPos::from_dvec3(equiped_mc_entity.position());

        // Send packets to all clients which have the equiped entity in view
        for (client_entity, mut client) in &mut clients {
            if client.instance() != equiped_instance {
                continue;
            }

            // It is not necessary to `SetEquipment` packets for the associate client's player entity,
            // because its equipments are already tracked on the client side.
            if client_entity != equiped_entity && client.view().contains(equiped_chunk_pos) {
                client.write_packet(&SetEquipment {
                    entity_id: VarInt(equiped_mc_entity.protocol_id()),
                    equipment: equips.iter_modified_equipments().collect(),
                });
            }
        }

        equips.clear_modified_slot();
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn modify_equipments() {
        let mut equipments = Equipments::default();
        assert_eq!(
            equipments,
            Equipments {
                equipments: [None, None, None, None, None, None],
                modified_slots: 0
            }
        );

        let item = ItemStack::new(ItemKind::GreenWool, 1, None);
        let slot = EquipmentSlot::Boots;
        equipments.set(item.clone(), slot);

        if let Some(equip) = equipments.get(EquipmentSlot::Boots) {
            assert_eq!(
                equip,
                Box::new(EquipmentEntry {
                    slot: slot.into(),
                    item: Some(item)
                })
            );
        }

        assert_eq!(
            equipments,
            Equipments {
                equipments: [
                    None,
                    None,
                    Some(Box::new(EquipmentEntry {
                        slot: slot.into(),
                        item: Some(item)
                    })),
                    None,
                    None,
                    None
                ],
                modified_slots: 0b100
            }
        );

        equipments.clear_modified_slot();
        equipments.clear();
        assert_eq!(
            equipments,
            Equipments {
                equipments: [None, None, None, None, None, None],
                modified_slots: 0b100
            }
        );
        assert_eq!(
            equipments
                .iter_modified_equipments()
                .collect::<Vec<EquipmentEntry>>(),
            vec![EquipmentEntry {
                slot: slot.into(),
                item: None
            }]
        );
    }
}
