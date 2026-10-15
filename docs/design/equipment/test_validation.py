"""Mutation scenarios for the proposal validator, not gameplay tests."""
import copy
import json
from pathlib import Path
import unittest
import validate


class ValidationTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parent / 'data'
        files = ('catalogue.json', 'loadouts.json', 'instance-example.json')
        self.data = [json.loads((root / name).read_text()) for name in files]
        self.c, self.l, self.i = self.data

    def reject(self, path):
        with self.assertRaisesRegex(validate.Invalid, path):
            validate.validate(*self.data)

    def test_valid_package(self):
        _, count, rows = validate.validate(*self.data)
        self.assertEqual(count, 266)
        self.assertEqual(len(rows), len(self.l["loadouts"]))

    def test_unknown_maker(self):
        self.c['weapons'][0]['maker'] = 'missing'
        self.reject('maker')

    def test_nonfinite_factor(self):
        self.c['modules'][0]['factors']['aim_spread'] = float('nan')
        self.reject('aim_spread')

    def test_boolean_factor(self):
        self.c['modules'][0]['factors']['aim_spread'] = True
        self.reject('aim_spread')

    def test_wrong_metric_scope(self):
        self.c['modules'][0]['factors']['sprint_speed'] = 0.9
        self.reject('sprint_speed')

    def test_slot_collision(self):
        self.l['loadouts'][1]['modules'].append('veyra:module.adapted_morrow_optic')
        self.reject('slots')

    def test_wrong_interface(self):
        self.c['modules'][0]['interface'] = 'veyra:interface.precision_rifle_sight'
        self.reject('interface')

    def test_wrong_revision(self):
        self.c['modules'][0]['interface_revision'] = 2
        self.reject('revision')

    def test_duplicate_inventory_install(self):
        duplicate = copy.deepcopy(self.i['items'][0])
        duplicate['instance_id'] = 'rifle_002'
        self.i['items'].append(duplicate)
        self.reject('installed more than once')

    def test_orphan_inventory_install(self):
        self.i['items'][0]['installed_modules'] = {}
        self.reject('orphan')

    def test_nonreciprocal_install(self):
        self.i['items'][1]['location']['parent'] = 'wrong'
        self.reject('reciprocal')

    def test_implant_overflow(self):
        self.l['loadouts'][0]['implants'] = [r['id'] for r in self.c['implants']]
        self.reject('capacity exceeded')

    def test_duplicate_supply_rows(self):
        rows = self.l['loadouts'][0]['supplies'] = [{'item': self.c['supplies'][0]['id'], 'count': 2}]
        rows.append(copy.deepcopy(rows[0]))
        self.reject('duplicate')

    def test_supply_stack(self):
        self.l['loadouts'][0]['supplies'] = [{'item': self.c['supplies'][0]['id'], 'count': 6}]
        self.reject('stack limit')

    def test_supply_total(self):
        self.l['loadouts'][0]['supplies'] = [{'item': s['id'], 'count': 3} for s in self.c['supplies']]
        self.reject('six ready')

    def test_boolean_count(self):
        self.l['loadouts'][0]['supplies'] = [{'item': self.c['supplies'][0]['id'], 'count': True}]
        self.reject('count')

    def test_composed_bound_for_unlisted_build(self):
        # Each source is valid alone; an optional optic plus fore-end exceeds cap.
        cooled = next(m for m in self.c['modules'] if m['id'].endswith('rifle_cooled_fore_end'))
        cooled['factors']['equip_time'] = 1.3
        self.reject('combinations.*equip_time')

    def test_negative_pool(self):
        self.i['character_state'][0]['current_pools']['health'] = -1
        self.reject('health')

    def test_boolean_cooldown(self):
        self.i['character_state'][0]['cooldowns_remaining_sim_seconds']['recovery_supply'] = False
        self.reject('cooldowns')

    def test_stock_not_one(self):
        self.c['weapons'][0]['stock_indices']['shot_damage'] = 1.1
        self.reject('shot_damage')

    def test_benefit_without_cost(self):
        self.c['modules'][0]['factors'] = {'aim_spread': 0.9}
        self.reject('benefit and cost')

    def test_missing_field(self):
        del self.c['weapons'][0]['maker']
        self.reject('weapons.*maker.*missing')

    def test_wrong_container(self):
        self.l['loadouts'][0]['modules'] = {}
        self.reject('modules.*expected list')

    def test_empty_factors(self):
        self.c['modules'][0]['factors'] = {}
        self.reject('benefit and cost')

    def test_negative_factor(self):
        self.c['implants'][0]['actor_factors']['sprint_speed'] = -1
        self.reject('sprint_speed')

    def test_implant_composed_bound(self):
        self.c['implants'][0]['weapon_factors']['recoil_recovery'] = 0.65
        self.reject('combinations.*recoil_recovery')


if __name__ == '__main__':
    unittest.main()
