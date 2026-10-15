#!/usr/bin/env python3
"""Validate the equipment paper package, never simulate combat."""
import argparse
import itertools
import json
import math
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent
WEAPON_HIGH = {
    'shot_damage', 'shot_posture', 'capacity', 'aim_move_speed',
    'cooling_rate', 'hit_damage', 'hit_posture',
}
WEAPON_LOW = {
    'repeat_interval', 'reload_time', 'aim_spread', 'equip_time',
    'recoil_recovery', 'heat_per_shot', 'brace_time',
    'attack_stamina', 'recovery_time',
}
ACTOR_HIGH = {'sprint_speed', 'stamina_capacity'}
ACTOR_LOW = {'dodge_stamina', 'shield_recharge_delay'}


class Invalid(ValueError):
    """An actionable package validation failure."""


def require(ok, path, message):
    if not ok:
        raise Invalid(f'{path}: {message}')


def number(value, path, minimum=0, integer=False):
    require(
        type(value) in (int, float) and math.isfinite(value),
        path,
        'expected finite number (not bool)',
    )
    require(not integer or type(value) is int, path, 'expected integer')
    require(value >= minimum, path, f'must be >= {minimum}')
    return value


def unique(values, path):
    require(len(values) == len(set(values)), path, 'duplicate value')


def index(rows, path, key='id'):
    require(isinstance(rows, list), path, 'expected list')
    result = {}
    for n, row in enumerate(rows):
        ident = row[key]
        require(isinstance(ident, str) and ident, f'{path}[{n}].{key}', 'expected nonempty string')
        require(ident not in result, f'{path}[{n}].{key}', f'duplicate {ident}')
        result[ident] = row
    return result


def reference(value, mapping, path):
    require(isinstance(value, str) and value in mapping, path, f'unknown reference {value!r}')
    return mapping[value]


def _validate(c, l, instance):
    for label, document in [('catalogue', c), ('loadouts', l), ('instance', instance)]:
        require(
            document['schema_version'] == 1 and type(document['schema_version']) is int,
            label+'.schema_version',
            'expected version 1',
        )
        require(
            document['status'] == 'design_proposal_not_runtime',
            label+'.status',
            'must remain a design proposal',
        )
    sections = (
        'makers', 'interfaces', 'certifications', 'weapons',
        'modules', 'implants', 'supplies',
    )
    maps = {
        name: index(c[name], 'catalogue.' + name)
        for name in sections
    }
    all_ids = [ident for mapping in maps.values() for ident in mapping]
    unique(all_ids, 'catalogue IDs')
    rules = c['rules']
    for field, expected in [('module_factor_bounds', [0.65, 1.35]), ('composed_index_bounds', [0.65, 1.35])]:
        for n, value in enumerate(rules[field]):
            number(value, f'rules.{field}[{n}]')
        require(rules[field] == expected, 'rules.'+field, f'expected {expected}')
    for field, expected in [('implant_capacity', 4), ('ready_supply_limit', 6)]:
        require(
            number(rules[field], 'rules.'+field, integer=True) == expected,
            'rules.'+field,
            f'expected {expected}',
        )
    metrics = c['metrics']
    expected_metrics = WEAPON_HIGH | WEAPON_LOW | ACTOR_HIGH | ACTOR_LOW
    require(set(metrics) == expected_metrics, 'metrics', 'unsupported or missing metric')
    for name, row in metrics.items():
        scope = 'actor' if name in ACTOR_HIGH | ACTOR_LOW else 'weapon'
        direction = 'higher' if name in WEAPON_HIGH | ACTOR_HIGH else 'lower'
        require(
            row['scope'] == scope and row['benefit_direction'] == direction,
            'metrics.'+name,
            'incorrect scope or benefit direction',
        )

    def factors(values, scope, path, stock=False):
        for metric, value in values.items():
            row = reference(metric, metrics, path+'.'+metric)
            require(row['scope'] == scope, path+'.'+metric, 'wrong metric scope')
            number(value, path+'.'+metric)
            require(
                value == 1 if stock else 0.65 - 1e-9 <= value <= 1.35 + 1e-9,
                path+'.'+metric,
                'stock must be 1; factors must be 0.65..1.35',
            )
    factors(rules['actor_stock_indices'], 'actor', 'rules.actor_stock_indices', True)
    require(
        set(rules['actor_stock_indices']) == ACTOR_HIGH | ACTOR_LOW,
        'rules.actor_stock_indices',
        'missing actor index',
    )
    for ident, row in maps['interfaces'].items():
        number(row['revision'], ident+'.revision', 1, True)
    stock_ids = []
    for ident, weapon in maps['weapons'].items():
        reference(weapon['maker'], maps['makers'], ident+'.maker')
        reference(weapon['certification'], maps['certifications'], ident+'.certification')
        require(weapon['family'] in ('ranged', 'melee'), ident+'.family', 'unknown family')
        number(weapon['nominal_length_m'], ident+'.nominal_length_m', 0.000001)
        factors(weapon['stock_indices'], 'weapon', ident+'.stock_indices', True)
        require(
            weapon['absolute_profile'] is None,
            ident+'.absolute_profile',
            'absolute balance profile is outside this draft',
        )
        for slot, fit in weapon['slots'].items():
            interface = reference(fit['interface'], maps['interfaces'], ident+'.slots.'+slot)
            require(interface['slot'] == slot, ident+'.slots.'+slot, 'interface slot mismatch')
            stock_ids.append(fit['stock_part'])
    unique(stock_ids + all_ids, 'stock part IDs')

    def tradeoff(values, path):
        changes = [
            (value - 1)
            * (1 if metrics[name]['benefit_direction'] == 'higher' else -1)
            for name, value in values.items()
        ]
        require(
            any(v > 0 for v in changes) and any(v < 0 for v in changes),
            path,
            'requires both benefit and cost',
        )

    def compatible(module, weapon, path):
        require(
            weapon['id'] in module['compatible_chassis'],
            path,
            'module does not allow this chassis',
        )
        slot = module['slot']
        require(slot in weapon['slots'], path, 'module slot absent from chassis')
        fit = weapon['slots'][slot]
        require(fit['interface'] == module['interface'], path, 'wrong interface')
        interface = maps['interfaces'][fit['interface']]
        require(
            module['interface_revision'] == interface['revision'],
            path,
            'wrong interface revision',
        )
    for ident, module in maps['modules'].items():
        reference(module['maker'], maps['makers'], ident+'.maker')
        reference(module['interface'], maps['interfaces'], ident+'.interface')
        number(module['interface_revision'], ident+'.interface_revision', 1, True)
        require(module['compatible_chassis'], ident+'.compatible_chassis', 'empty allowlist')
        unique(module['compatible_chassis'], ident+'.compatible_chassis')
        factors(module['factors'], 'weapon', ident+'.factors')
        tradeoff(module['factors'], ident+'.factors')
        for chassis in module['compatible_chassis']:
            weapon = reference(chassis, maps['weapons'], ident+'.compatible_chassis')
            compatible(module, weapon, ident)
            require(
                set(module['factors']) <= set(weapon['stock_indices']),
                ident+'.factors',
                'metric absent from chassis',
            )
    for ident, implant in maps['implants'].items():
        reference(implant['maker'], maps['makers'], ident+'.maker')
        number(implant['capacity_cost'], ident+'.capacity_cost', 1, True)
        require(
            implant['weapon_scope'] in ('ranged', 'melee', 'all'),
            ident+'.weapon_scope',
            'unknown scope',
        )
        require(
            implant['unique_per_character'] is True,
            ident+'.unique_per_character',
            'must be unique',
        )
        factors(implant['weapon_factors'], 'weapon', ident+'.weapon_factors')
        factors(implant['actor_factors'], 'actor', ident+'.actor_factors')
        tradeoff(implant['weapon_factors'] | implant['actor_factors'], ident)
        for weapon in maps['weapons'].values():
            if implant['weapon_scope'] in ('all', weapon['family']):
                require(
                    set(implant['weapon_factors']) <= set(weapon['stock_indices']),
                    ident+'.weapon_factors',
                    'metric absent from applicable chassis',
                )
    for ident, supply in maps['supplies'].items():
        reference(supply['maker'], maps['makers'], ident+'.maker')
        require(
            supply['restore']['pool'] in ('health', 'shield', 'stamina'),
            ident+'.restore.pool',
            'unknown pool',
        )
        number(supply['restore']['amount'], ident+'.restore.amount', 0.000001)
        number(supply['use_seconds'], ident+'.use_seconds', 0.000001)
        require(
            number(supply['cooldown_seconds'], ident + '.cooldown_seconds') == 45
            and supply['cooldown_group'] == 'recovery_supply',
            ident,
            'requires shared 45s recovery cooldown',
        )
        require(
            number(supply['stack_limit'], ident+'.stack_limit', 1, True) == 5,
            ident+'.stack_limit',
            'expected five',
        )

    def compose(weapon, modules, implants, path):
        result = {'weapon': dict(weapon['stock_indices']), 'actor': dict(rules['actor_stock_indices'])}
        for module in sorted(modules, key=lambda row: row['id']):
            for name, value in module['factors'].items():
                result['weapon'][name] *= value
        for implant in sorted(implants, key=lambda row: row['id']):
            for name, value in implant['actor_factors'].items():
                result['actor'][name] *= value
            if implant['weapon_scope'] in ('all', weapon['family']):
                for name, value in implant['weapon_factors'].items():
                    result['weapon'][name] *= value
        for scope, values in result.items():
            for name, value in values.items():
                require(
                    math.isfinite(value) and 0.65 - 1e-9 <= value <= 1.35 + 1e-9,
                    path+'.'+scope+'.'+name,
                    f'composed factor {value:.6g} outside 0.65..1.35',
                )
        return result
    implant_sets = [
        list(combo)
        for n in range(len(maps['implants']) + 1)
        for combo in itertools.combinations(maps['implants'].values(), n)
        if sum(i['capacity_cost'] for i in combo) <= 4
    ]
    exhaustive = 0
    for weapon in maps['weapons'].values():
        choices = [
            [None] + [
                module for module in maps['modules'].values()
                if weapon['id'] in module['compatible_chassis']
                and module['slot'] == slot
            ]
            for slot in weapon['slots']
        ]
        for combo in itertools.product(*choices):
            modules = [m for m in combo if m]
            for implants in implant_sets:
                compose(
                    weapon,
                    modules,
                    implants,
                    'combinations.'+weapon['id']+'['+','.join(m['id'] for m in modules+implants)+']',
                )
                exhaustive += 1

    def supplies(rows, path):
        unique([r['item'] for r in rows], path)
        total = 0
        for n, row in enumerate(rows):
            supply = reference(row['item'], maps['supplies'], f'{path}[{n}].item')
            count = number(row['count'], f'{path}[{n}].count', 1, True)
            require(count <= supply['stack_limit'], f'{path}[{n}].count', 'exceeds stack limit')
            total += count
        require(total <= 6, path, 'exceeds six ready doses')
    table = []
    for ident, loadout in index(l['loadouts'], 'loadouts').items():
        weapon = reference(loadout['weapon'], maps['weapons'], ident+'.weapon')
        unique(loadout['modules'], ident+'.modules')
        unique(loadout['implants'], ident+'.implants')
        modules = [reference(m, maps['modules'], ident+'.modules') for m in loadout['modules']]
        implants = [reference(i, maps['implants'], ident+'.implants') for i in loadout['implants']]
        unique([m['slot'] for m in modules], ident+'.modules slots')
        for module in modules:
            compatible(module, weapon, ident + '.modules')
        require(
            sum(i['capacity_cost'] for i in implants) <= 4,
            ident+'.implants',
            'implant capacity exceeded',
        )
        supplies(loadout['supplies'], ident+'.supplies')
        table.append((ident, compose(weapon, modules, implants, ident)))
    require(
        instance['example_only'] is True,
        'instance.example_only',
        'incomplete example must be marked example-only',
    )
    characters = instance['characters']
    unique(characters, 'instance.characters')
    require(
        instance['active_character'] in characters,
        'instance.active_character',
        'unknown character',
    )
    items = index(instance['items'], 'instance.items', 'instance_id')
    definitions = maps['weapons'] | maps['modules'] | maps['implants'] | maps['supplies']
    installs = {}
    ready = {char: [] for char in characters}
    implant_costs = {char: [] for char in characters}
    for ident, item in items.items():
        definition = reference(item['definition'], definitions, ident+'.definition')
        location = item['location']
        kind = location['kind']
        allowed_keys = {
            'equipped': {'kind', 'character'},
            'ready': {'kind', 'character'},
            'installed': {'kind', 'parent', 'slot'},
            'storage': {'kind'},
        }
        require(
            kind in allowed_keys and set(location) == allowed_keys[kind],
            ident+'.location',
            'location must describe exactly one ownership placement',
        )
        require(
            kind in ('equipped', 'installed', 'ready', 'storage'),
            ident+'.location.kind',
            'unknown location',
        )
        if kind in ('equipped', 'ready'):
            require(
                location['character'] in characters,
                ident+'.location.character',
                'unknown character',
            )
        if kind == 'storage':
            require(
                not ({'parent', 'character', 'slot'} & location.keys()),
                ident+'.location',
                'storage must not also assign an owner or parent',
            )
        if 'provenance' in item:
            reference(
                item['provenance']['maker'],
                maps['makers'],
                ident + '.provenance.maker',
            )
        if item['definition'] in maps['weapons']:
            require(
                kind in ('equipped', 'storage'),
                ident+'.location',
                'weapon must be equipped or stored',
            )
            require(
                item['stock_parts'] == 'integral_retained',
                ident+'.stock_parts',
                'requires retained stock parts',
            )
            for field in ('loaded_ammo', 'spare_ammo'):
                number(item[field], ident + '.' + field, integer=True)
            number(item['heat'], ident+'.heat')
            for slot, child_id in item['installed_modules'].items():
                child = reference(child_id, items, ident+'.installed_modules.'+slot)
                require(
                    child_id not in installs,
                    ident+'.installed_modules.'+slot,
                    'instance installed more than once',
                )
                installs[child_id] = (ident, slot)
                module = reference(child['definition'], maps['modules'], child_id+'.definition')
                compatible(module, definition, ident+'.installed_modules.'+slot)
                require(
                    module['slot'] == slot
                    and child['location'] == {
                        'kind': 'installed', 'parent': ident, 'slot': slot,
                    },
                    child_id+'.location',
                    'install must be reciprocal with matching slot',
                )
        elif item['definition'] in maps['supplies']:
            require(
                kind in ('ready', 'storage'),
                ident+'.location',
                'supply must be ready or stored',
            )
            quantity = number(item['quantity'], ident+'.quantity', 1, True)
            require(quantity <= definition['stack_limit'], ident+'.quantity', 'exceeds stack limit')
            if kind == 'ready':
                ready[location['character']].append({
                    'item': item['definition'],
                    'count': quantity,
                })
        elif item['definition'] in maps['implants']:
            require(
                kind in ('equipped', 'storage'),
                ident+'.location',
                'implant must be equipped or stored',
            )
            if kind == 'equipped':
                implant_costs[location['character']].append(definition)
        else:
            require(
                kind in ('installed', 'storage'),
                ident+'.location',
                'module must be installed or stored',
            )
    for ident, item in items.items():
        if item['location']['kind'] == 'installed':
            require(ident in installs, ident + '.location', 'orphan installed instance')
    for char in characters:
        supplies(ready[char], 'instance.ready.'+char)
        unique([i['id'] for i in implant_costs[char]], 'instance.implants.'+char)
        require(
            sum(i['capacity_cost'] for i in implant_costs[char]) <= 4,
            'instance.implants.'+char,
            'implant capacity exceeded',
        )
    for ident, item in items.items():
        if item['definition'] in maps['weapons']:
            modules = [
                maps['modules'][items[child]['definition']]
                for child in item['installed_modules'].values()
            ]
            implants = (
                implant_costs[item['location']['character']]
                if item['location']['kind'] == 'equipped'
                else []
            )
            compose(maps['weapons'][item['definition']], modules, implants, 'instance.'+ident)
    states = index(instance['character_state'], 'instance.character_state', 'character')
    require(
        set(states) == set(characters),
        'instance.character_state',
        'requires one state per character',
    )
    for char, state in states.items():
        require(
            set(state['current_pools']) == {'health', 'shield', 'stamina', 'posture'},
            char+'.current_pools',
            'missing or unknown pool',
        )
        for name, value in state['current_pools'].items():
            number(value, char + '.current_pools.' + name)
        require(
            set(state['cooldowns_remaining_sim_seconds']) == {'recovery_supply'},
            char+'.cooldowns',
            'requires recovery supply cooldown',
        )
        number(
            state['cooldowns_remaining_sim_seconds']['recovery_supply'],
            char+'.cooldowns.recovery_supply',
        )
    return maps, exhaustive, table


def _shape_and_validate(c, l, instance):
    """Check the draft contract; this is not a complete JSON Schema validator."""
    def shape(value, template, path):
        if isinstance(template, dict):
            require(isinstance(value, dict), path, 'expected object')
            for key, child in template.items():
                require(key in value, path+'.'+key, 'missing required field')
                shape(value[key], child, path+'.'+key)
        elif isinstance(template, list):
            require(isinstance(value, list), path, 'expected list')
            if template:
                for n, child in enumerate(value):
                    shape(child, template[0], f'{path}[{n}]')
    common = {'schema_version': None, 'status': None}
    catalogue_shape = common | {
        'rules': {
            'module_factor_bounds': [],
            'composed_index_bounds': [],
            'implant_capacity': None,
            'ready_supply_limit': None,
            'actor_stock_indices': {},
        },
        'metrics': {},
        'makers': [{'id': None}],
        'interfaces': [{'id': None, 'revision': None, 'slot': None}],
        'certifications': [{'id': None}],
        'weapons': [{
            'id': None,
            'maker': None,
            'certification': None,
            'family': None,
            'nominal_length_m': None,
            'slots': {},
            'stock_indices': {},
            'absolute_profile': None,
        }],
        'modules': [{
            'id': None,
            'maker': None,
            'slot': None,
            'interface': None,
            'interface_revision': None,
            'compatible_chassis': [],
            'factors': {},
        }],
        'implants': [{
            'id': None,
            'maker': None,
            'capacity_cost': None,
            'weapon_scope': None,
            'unique_per_character': None,
            'weapon_factors': {},
            'actor_factors': {},
        }],
        'supplies': [{
            'id': None,
            'maker': None,
            'restore': {'pool': None, 'amount': None},
            'use_seconds': None,
            'cooldown_seconds': None,
            'cooldown_group': None,
            'stack_limit': None,
        }],
    }
    shape(c, catalogue_shape, 'catalogue')
    for name, metric in c['metrics'].items():
        shape(
            metric,
            {'scope': None, 'benefit_direction': None},
            'metrics.' + name,
        )
    for weapon in c['weapons']:
        for slot, fit in weapon['slots'].items():
            shape(
                fit,
                {'interface': None, 'stock_part': None},
                weapon['id'] + '.slots.' + slot,
            )
    loadout_shape = common | {
        'loadouts': [{
            'id': None,
            'weapon': None,
            'modules': [],
            'implants': [],
            'supplies': [{'item': None, 'count': None}],
        }],
    }
    shape(l, loadout_shape, 'loadouts')
    instance_shape = common | {
        'example_only': None,
        'characters': [],
        'active_character': None,
        'items': [{
            'instance_id': None,
            'definition': None,
            'location': {'kind': None},
        }],
        'character_state': [{
            'character': None,
            'current_pools': {},
            'cooldowns_remaining_sim_seconds': {},
        }],
    }
    shape(instance, instance_shape, 'instance')
    for item in instance['items']:
        ident = item['instance_id']
        location = item['location']
        if location['kind'] in ('equipped', 'ready'):
            shape(location, {'character': None}, ident + '.location')
        if location['kind'] == 'installed':
            shape(
                location,
                {'parent': None, 'slot': None},
                ident + '.location',
            )
        if item['definition'] in [row['id'] for row in c['weapons']]:
            shape(item, {
                'stock_parts': None,
                'loaded_ammo': None,
                'spare_ammo': None,
                'heat': None,
                'installed_modules': {},
            }, ident)
        if item['definition'] in [row['id'] for row in c['supplies']]:
            shape(item, {'quantity': None}, ident)
    try:
        return _validate(c, l, instance)
    except (KeyError, TypeError, AttributeError) as error:
        raise Invalid(f'package structure: malformed field or container ({error})') from error


def validate(c, l, instance):
    """Expose all malformed draft inputs as actionable validation failures."""
    try:
        return _shape_and_validate(c, l, instance)
    except (KeyError, TypeError, AttributeError, IndexError) as error:
        raise Invalid(f'package structure: malformed field or container ({error})') from error


def report(result):
    maps, combinations, table = result
    weapon_configurations = sum(
        math.prod(
            1 + sum(
                weapon['id'] in module['compatible_chassis']
                and module['slot'] == slot
                for module in maps['modules'].values()
            )
            for slot in weapon['slots']
        )
        for weapon in maps['weapons'].values()
    )
    implant_subsets = combinations // weapon_configurations
    summary = (
        f'Passed: {len(maps["weapons"])} chassis, '
        f'{len(maps["modules"])} modules, '
        f'{len(maps["implants"])} implants, '
        f'{len(maps["supplies"])} supplies; {len(table)} loadouts; '
        f'{combinations} exhaustive chassis/module/implant configurations '
        f'({weapon_configurations} weapon configurations × '
        f'{implant_subsets} legal implant subsets for this roster).'
    )
    limitations = (
        'All indices are relative to that chassis stock, never cross-chassis '
        'power or measured balance. Instance example is incomplete runtime '
        'persistence: no absolute capacities, certifications, world state or '
        'complete server transaction history.'
    )
    lines = [
        '# Equipment data validation',
        '',
        summary,
        '',
        limitations,
        '',
        '| Configuration | Changed weapon factors | Changed actor factors |',
        '| --- | --- | --- |',
    ]
    for ident, values in table:
        cells = [
            ', '.join(
                f'{name}={value:.4g}'
                for name, value in scope.items()
                if value != 1
            ) or 'stock (1)'
            for scope in values.values()
        ]
        lines.append('| ' + ident + ' | ' + ' | '.join(cells) + ' |')
    return '\n'.join(lines)+'\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, help='write Markdown to this explicit path')
    args = parser.parse_args()
    try:
        files = ('catalogue.json', 'loadouts.json', 'instance-example.json')
        data = [
            json.loads((ROOT / 'data' / name).read_text())
            for name in files
        ]
        output = report(validate(*data))
        if args.report:
            args.report.write_text(output)
        print(output, end='')
    except (Invalid, KeyError, TypeError, AttributeError, OSError, json.JSONDecodeError) as error:
        print(f'Validation failed: {error}', file=sys.stderr)
        return 1
    return 0

if __name__ == '__main__':
    sys.exit(main())
