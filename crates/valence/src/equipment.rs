use bevy_ecs::prelude::*;
use bevy_ecs::{query::Changed, system::Query};
use valence_protocol::packets::s2c::set_equipment::SetEquipment;
use valence_protocol::VarInt;
use valence_protocol::{packets::s2c::set_equipment::EquipmentEntry, ItemStack};

use crate::prelude::*;
use crate::view::ChunkPos;

/// ECS component to be added for entities with equipments.
///
/// Equipment updates managed by [update_equipment].
#[derive(Component, Default, PartialEq, Debug)]
pub struct Equipments {
    equipments: Vec<EquipmentEntry>,
    /// Bit set with the modified equipment slots
    modified_slots: u8,
}

#[derive(Copy, Clone)]
pub enum EquipmentSlot {
    MainHand,
    OffHand,
    Boots,
    Leggings,
    Chestplate,
    Helmet,
}

impl Equipments {
    pub fn new() -> Equipments {
        Equipments::default()
    }

    /// Set an equipment slot with an item stack
    pub fn set(&mut self, item: ItemStack, slot: EquipmentSlot) {
        if let Some(equip) = self.get_mut(slot) {
            equip.item = Some(item);
        } else {
            self.equipments.push(EquipmentEntry {
                item: Some(item),
                slot: slot.into(),
            });
        }

        self.set_modified_slot(slot);
    }

    /// Remove all equipments
    pub fn clear(&mut self) {
        for equip in self.equipments.iter() {
            self.modified_slots |= 1 << equip.slot as u8;
        }

        self.equipments.clear();
    }

    /// Remove an equipment from a slot and return it if present
    pub fn remove(&mut self, slot: EquipmentSlot) -> Option<EquipmentEntry> {
        let slot: i8 = slot.into();

        if let Some(idx) = self.equipments.iter().position(|equip| equip.slot == slot) {
            Some(self.equipments.remove(idx))
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, slot: EquipmentSlot) -> Option<&mut EquipmentEntry> {
        let slot: i8 = slot.into();
        self.equipments.iter_mut().find(|equip| equip.slot == slot)
    }

    pub fn get(&self, slot: EquipmentSlot) -> Option<&EquipmentEntry> {
        let slot: i8 = slot.into();
        self.equipments.iter().find(|equip| equip.slot == slot)
    }

    pub fn equipments(&self) -> &Vec<EquipmentEntry> {
        &self.equipments
    }

    pub fn is_empty(&self) -> bool {
        self.equipments.is_empty()
    }

    fn has_modified_slots(&self) -> bool {
        self.modified_slots != 0
    }

    fn iter_modified_equipments(&self) -> impl Iterator<Item = EquipmentEntry> + '_ {
        self.iter_modified_slots().map(|slot| {
            self.get(slot).cloned().unwrap_or_else(|| EquipmentEntry {
                slot: slot.into(),
                item: None,
            })
        })
    }

    fn iter_modified_slots(&self) -> impl Iterator<Item = EquipmentSlot> {
        let modified_slots = self.modified_slots;

        (0..=5).filter_map(move |slot: i8| {
            if modified_slots & (1 << slot) != 0 {
                Some(EquipmentSlot::try_from(slot).unwrap())
            } else {
                None
            }
        })
    }

    fn set_modified_slot(&mut self, slot: EquipmentSlot) {
        let shifts: i8 = slot.into();
        self.modified_slots |= 1 << (shifts as u8);
    }

    fn clear_modified_slot(&mut self) {
        self.modified_slots = 0;
    }
}

impl TryFrom<i8> for EquipmentSlot {
    type Error = &'static str;

    /// Convert from `id` according to https://wiki.vg/Protocol#Set_Equipment
    fn try_from(id: i8) -> Result<Self, Self::Error> {
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

impl From<EquipmentSlot> for i8 {
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
                equipments: vec![],
                modified_slots: 0
            }
        );

        let item = ItemStack::new(ItemKind::GreenWool, 1, None);
        let slot = EquipmentSlot::Boots;
        equipments.set(item.clone(), slot);

        assert_eq!(
            equipments,
            Equipments {
                equipments: vec![EquipmentEntry {
                    slot: slot.into(),
                    item: Some(item)
                }],
                modified_slots: 0b100
            }
        );

        equipments.clear_modified_slot();
        equipments.clear();
        assert_eq!(
            equipments,
            Equipments {
                equipments: vec![],
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
