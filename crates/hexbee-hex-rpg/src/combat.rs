use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::weapons::{weapon_damage_range, Weapon};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Combatant {
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub weapon: Weapon,
}

impl Combatant {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackResult {
    pub attacker: String,
    pub defender: String,
    pub hit: bool,
    pub damage: u8,
    pub defender_hp_after: i32,
}

/// 50% hit chance; damage from weapon dice. Unarmed deals 0 on a "hit".
pub fn resolve_attack<R: Rng>(
    attacker: &Combatant,
    defender: &mut Combatant,
    rng: &mut R,
) -> AttackResult {
    let hit = rng.gen_bool(0.5);
    let mut damage = 0u8;
    if hit {
        if let Some((lo, hi)) = weapon_damage_range(attacker.weapon) {
            damage = rng.gen_range(lo..=hi);
            defender.hp -= damage as i32;
        }
    }
    AttackResult {
        attacker: attacker.name.clone(),
        defender: defender.name.clone(),
        hit,
        damage,
        defender_hp_after: defender.hp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn knife_damage_in_range_when_hit() {
        let mut rng = StdRng::seed_from_u64(7);
        let attacker = Combatant {
            name: "P".into(),
            hp: 10,
            max_hp: 10,
            weapon: Weapon::Knife,
        };
        let mut hits = 0;
        for _ in 0..40 {
            let mut defender = Combatant {
                name: "E".into(),
                hp: 8,
                max_hp: 8,
                weapon: Weapon::Knife,
            };
            let r = resolve_attack(&attacker, &mut defender, &mut rng);
            if r.hit {
                hits += 1;
                assert!((1..=3).contains(&r.damage));
                assert_eq!(defender.hp, 8 - r.damage as i32);
            } else {
                assert_eq!(r.damage, 0);
                assert_eq!(defender.hp, 8);
            }
        }
        assert!(hits > 0);
    }
}
