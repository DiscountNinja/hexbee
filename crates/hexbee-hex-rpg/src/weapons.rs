use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Weapon {
    Knife,
    Mace,
    BattleAxe,
    RoyalSword,
    None,
}

impl Weapon {
    pub fn display_name(self) -> &'static str {
        match self {
            Weapon::Knife => "knife",
            Weapon::Mace => "mace",
            Weapon::BattleAxe => "battle axe",
            Weapon::RoyalSword => "royal sword",
            Weapon::None => "unarmed",
        }
    }
}

/// Inclusive damage range for a weapon.
pub fn weapon_damage_range(weapon: Weapon) -> Option<(u8, u8)> {
    match weapon {
        Weapon::Knife => Some((1, 3)),
        Weapon::Mace | Weapon::BattleAxe | Weapon::RoyalSword => Some((2, 6)),
        Weapon::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knife_and_large_weapons() {
        assert_eq!(weapon_damage_range(Weapon::Knife), Some((1, 3)));
        assert_eq!(weapon_damage_range(Weapon::Mace), Some((2, 6)));
        assert_eq!(weapon_damage_range(Weapon::BattleAxe), Some((2, 6)));
        assert_eq!(weapon_damage_range(Weapon::RoyalSword), Some((2, 6)));
        assert_eq!(weapon_damage_range(Weapon::None), None);
    }
}
