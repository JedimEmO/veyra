//! The numbers that shape a world, gathered in one place so the map tool
//! and the tests can tune them without touching the stages.
//!
//! Everything a designer would call "the look of the planet" is here:
//! continent size, ocean depth, where the poles are. What a designer would
//! call "the look of a biome" is in the biome's own RON file. Defaults are
//! the ones `docs/research/worldmap-seed42.png` was rendered with.

/// Tunables of the continent, climate and blend stages.
///
/// Serialisable so [`world_version`](crate::world_version) can hash the
/// set in force; a new field joins the hash with no further change.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct WorldParams {
    /// Size of the largest continent feature, in metres. Continents come
    /// out on the order of one to two of these across.
    pub continent_wavelength_m: f64,
    /// Octaves of the continent field. More gives more ragged coasts.
    pub continent_octaves: usize,
    /// Size of the largest domain-warp feature, in metres.
    pub warp_wavelength_m: f64,
    /// How far the warp displaces a point, in metres. This is what turns
    /// blobs into peninsulas and inlets.
    pub warp_amplitude_m: f64,
    /// Added to the continent field before the land test. Positive means
    /// more land.
    pub land_bias: f64,
    /// Width of the shore step in continent-field units. The step lifts
    /// the elevation by `shore_step` across `-shore_width..=shore_width`,
    /// which is what keeps beaches and shelves narrow.
    pub shore_width: f64,
    /// Height of the shore step in normalised elevation. The coast biome's
    /// elevation window is authored to sit inside it.
    pub shore_step: f64,
    /// How much the continent field lifts the normalised elevation on land,
    /// per unit of the field. The lowland biomes' elevation windows are
    /// authored against this.
    pub land_rise: f64,
    /// Size of the largest mountain-range feature, in metres, and so about
    /// how far apart the ranges run: a range follows each zero contour of
    /// the signed range field, and the contours of a field this size are
    /// some tens of kilometres apart. 70 km until ticket 0004's step 2,
    /// which took 35 km with ridged noise (massifs); 47.5 km since its step
    /// 2b made the field signed, where ranges are long lines with wide
    /// foothills, ten degrees or more over the horizon from three fifths of
    /// the land, and the nearest one to Port Serein stands 9 km off.
    pub range_wavelength_m: f64,
    /// Octaves of the range field. Two since ticket 0004's step 2b, which
    /// made it read (the field had 3 whatever this said until then): a
    /// third put 19 % of the land in mountains and took the plains and
    /// forest round Port Serein under three fifths of its country.
    pub range_octaves: usize,
    /// Amplitude falloff per octave of the range field. Read since ticket
    /// 0004's step 2b, as `range_octaves` is.
    pub range_persistence: f64,
    /// Gain on the signed range noise before the rain march folds it into
    /// `1 - n^2`: at 2 (since ticket 0004's step 2b) the march's range stands
    /// wherever the field is within 0.5 of zero. Until step 4 the real
    /// continent read the same fold; since, it reads the distance to the
    /// field's zero contour ([`range_width_m`](Self::range_width_m)), and
    /// the march alone keeps the fold, which the rain was tuned on.
    pub range_sharpness: f64,
    /// How much longer than wide a range is: the range field is stretched
    /// by this along a seed-chosen axis. Seed 42's ranges run within about
    /// twenty degrees of its prevailing wind, so more stretch lays more of
    /// them along the wind, where they cast no rain shadow: at 3 the lee was
    /// drier at 10 of 26 summits (`tests/rainshadow.rs`), at 2.5 at 12 of 21
    /// (ticket 0004, step 2b).
    pub range_anisotropy: f64,
    /// The range's profile value (the fold for the march, how far in from
    /// the edge of [`range_width_m`](Self::range_width_m) for the continent)
    /// at which a range starts to rise. 0.87 until ticket 0004's step 2b,
    /// with a smoothstep of the fold; 0 since, the profile whole.
    pub range_floor: f64,
    /// The profile value at which a range reaches full `range_strength`.
    pub range_crest: f64,
    /// The power the profile, mapped from `range_floor..range_crest` to
    /// `0..=1`, is raised to: how long a foot a range stands on. 6 since
    /// ticket 0004's step 2b, on the fold of the field's value, where it
    /// made a bell; 3 since step 4, on the distance to the crest line,
    /// which falls about linearly: concave all the way to a crest rounded
    /// over [`range_round_m`](Self::range_round_m), with the mountains 11.9
    /// % of the land (a power of 2 put 16.8 % there at 12 km of width and
    /// eight flank profiles flagged; 4 put 3.6 % at 4 km).
    pub range_foot: usize,
    /// The power of the fold the rain march's coarse ranges are lifted by:
    /// 6, the bell ticket 0004's step 2b gave every range and tuned the rain
    /// on, kept for the march when step 4 read the continent's ranges from
    /// the distance to their crest lines. The crest lines are the same; the
    /// march reads no warp already, and the distance would need the field's
    /// gradient at every tap, 0.2 us a sample.
    pub rain_range_foot: usize,
    /// The continent field from which a range stands at its full height
    /// (over 0.02).
    /// Towards the coast it fades to nothing at the land's edge (a field of
    /// 0.02), by [`range_coast_power`](Self::range_coast_power). Until
    /// ticket 0004's review of step 2b the land mask did this, from 0.15,
    /// which cut a range meeting the coast off within about 1.3 km of the
    /// beach: a wall of up to 1 550 m in 340 m where the mountains' relief
    /// switched on across it. 0.28 since, with the power, keeps the ten
    /// degrees in view and the city's views that the ranges near a coast
    /// give; wider fades (0.35 and up) took them under their targets.
    pub range_coast: f64,
    /// How the coastal fade eases: the fade is `1 - (1 - s)^power` of a
    /// smoothstep `s` of the field from 0.02 to
    /// [`range_coast`](Self::range_coast). At 1 it is the smoothstep. Higher
    /// puts its steep part low on the flank, where the elevation is under
    /// the band in which the mountains' relief rises
    /// ([`mountain_rise`](Self::mountain_rise)), and leaves the top of the
    /// fade, where the relief switches on, gentle and long. 5 since ticket
    /// 0004's review of step 2b.
    pub range_coast_power: usize,
    /// How much a range adds to the normalised elevation at its crest. 0.42
    /// since ticket 0004's step 2b (0.35 before): with the bell's long foot
    /// less of each crest reaches where the mountains stand alone, and at
    /// 0.35 only 55 % of the land saw ten degrees. Lower is gentler flanks:
    /// the median of ten profiled flanks' largest rise in 250 m below half
    /// their height was 222 m at 0.35 and 562 m at 0.42 on step 2b's
    /// ground, and is 486 m since its review. The tallest of those walls
    /// are the coast's, not the strength's: where a range meets the shore,
    /// how fast it fades ([`range_coast`](Self::range_coast)) sets how fast
    /// the elevation crosses the band where the mountains' relief rises,
    /// and the two tallest, 1 001 and 899 m in 250 m (1 271 and 1 197 m
    /// before that review), stand where the continent field is under 0.15.
    /// A crest's elevation reaches its ceiling of 1 wherever the field is
    /// over 0.15. Since ticket 0004's step 4 the range's own metres
    /// ([`range_height_m`](Self::range_height_m)) carry most of its height
    /// and the median wall is 147 m; this sets the mountains' ground, which
    /// stands where the elevation is over 0.8.
    pub range_strength: f64,
    /// Depth of the deepest ocean floor below sea level, in metres.
    pub ocean_depth_m: f64,
    /// Height of the highest land base above sea level, in metres, before
    /// biome relief is added.
    pub land_base_m: f64,
    /// The `z` at which temperature peaks. -56 km since ticket 0004's step
    /// 2b (0 before, which put Port Serein, at `z` -500, on the hottest
    /// latitude there is, in desert and savanna): the city is now 55.5 km
    /// from it towards `+z`, half way to the pole, in the middle of the
    /// plains' and the forest's temperature windows.
    pub equator_z: f64,
    /// Distance from the equator at which the latitude temperature reaches
    /// zero, in metres. 110 km since ticket 0004's step 2b (55 km before):
    /// the temperate band round Port Serein is some 23 km deep either side
    /// of the city rather than 11, and within 100 km of the origin no
    /// biome takes more than a quarter of the land (tundra had 28.6 %, with
    /// the equator moved alone).
    pub pole_distance_m: f64,
    /// Temperature lost per metre of base elevation above sea level.
    pub lapse_per_m: f64,
    /// Peak-to-peak temperature noise, in normalised units.
    pub temperature_noise: f64,
    /// Size of the largest temperature-noise feature, in metres.
    pub temperature_wavelength_m: f64,
    /// Size of the largest moisture-noise feature, in metres.
    pub moisture_wavelength_m: f64,
    /// Gain on the moisture noise before clamping, so its extremes reach
    /// the ends of the unit range.
    pub moisture_stretch: f64,
    /// Size of the feature that turns the prevailing wind, in metres. Very
    /// large on purpose: the wind should be one direction over a continent
    /// and a different one a few hundred kilometres away, not a swirl.
    pub wind_turn_wavelength_m: f64,
    /// How far the turn noise may swing the wind, as the tangent of the
    /// angle. `0.35` is about twenty degrees either way, enough that a
    /// coastline is not uniformly windward along its whole length.
    pub wind_turn: f64,
    /// Taps the rain march evaluates upwind of the sample. This is the
    /// whole cost of rainfall, so it buys reach (`rain_taps` times
    /// `rain_tap_spacing_m`) at a fixed price per sample.
    pub rain_taps: usize,
    /// Metres between rain-march taps. Wide enough that six taps reach
    /// past a range, narrow enough that a range is not stepped over.
    pub rain_tap_spacing_m: f64,
    /// Octaves of the coarse continent the march walks through. The walk
    /// needs the land mask and the broad elevation only, so it gets a
    /// cheaper field than the real continent stage.
    pub rain_coarse_octaves: usize,
    /// Fraction of the carried moisture lost per tap over land, before any
    /// orographic rain-out. This is what dries a flat interior.
    pub rain_land_decay: f64,
    /// Fraction of the carried moisture that rains out per unit of
    /// normalised rise between two taps. Near one so a range empties the
    /// air crossing it and its lee is a shadow.
    pub rain_orographic_rate: f64,
    /// Metres of rise between two taps that counts as a full unit of
    /// orographic lift. Around the steepest a range's base climbs over one
    /// tap spacing, so a whole range rains itself out and a hill does not.
    pub rain_rise_scale_m: f64,
    /// Share of the climate's moisture that still comes from the old
    /// stretched noise rather than the march, so a flat interior with the
    /// same rainfall everywhere is not one uniform biome.
    pub rain_noise_share: f64,
    /// Radius of the four taps hydration compares the point against, in
    /// metres. The scale of a hollow that would hold water: a dell, not a
    /// footprint and not a valley.
    pub hydration_radius_m: f64,
    /// Metres the point may sit below its neighbourhood before the hollow
    /// term is at full strength.
    pub hydration_relief_scale_m: f64,
    /// How much a full hollow adds to hydration, and a full ridge takes
    /// away.
    pub hydration_hollow_gain: f64,
    /// Hydration lost per unit of slope, because water runs off a slope
    /// instead of soaking into it.
    pub hydration_slope_loss: f64,
    /// Hydration at which cover starts to rise off zero: drier than this
    /// is bare ground whatever the biome.
    pub cover_dry: f64,
    /// Hydration at which cover reaches the patchiness noise in full.
    pub cover_wet: f64,
    /// Size of the largest cover-patch feature, in metres: the scale of a
    /// grove and the clearing next to it.
    pub cover_wavelength_m: f64,
    /// Gain on the cover patch noise around `0.5` before clamping. Above
    /// one so a patch reaches bare ground and full cover rather than
    /// hovering around half.
    pub cover_contrast: f64,
    /// Score gap, in climate units, over which two biomes crossfade. The
    /// classifier's fields change by roughly this much over 500 m at a
    /// coast, which is the border width the plan asks for.
    pub blend_gap: f64,
    /// Normalised elevation over which a mountain biome's relief grows from
    /// nothing to its full amplitude, counted up from
    /// [`mountain_rise_lead`](Self::mountain_rise_lead) under where every
    /// other biome has left the blend.
    ///
    /// Not before: a biome's weight ramps over a band that can be 30 m
    /// wide where a range's flank is steep, and a relief hundreds of metres
    /// taller than its neighbour's, switched on across that band, is a
    /// scarp ringing every massif (ticket 0004's review of step 2). Wider
    /// is gentler flanks and lower mountains: at 0.06 to 0.08 the rise
    /// itself steepened into cliffs, and from 0.10 the view from the
    /// lowlands fell towards its floor (`tests/drama.rs`). 0.12 since
    /// ticket 0004's step 2b (0.09 before): with the lower relief that step
    /// gave the mountains, it is the widest rise that keeps ten degrees in
    /// view from three fifths of the land (0.18 dropped it to 59 %). 0.2
    /// since step 4, from 0.06 lower
    /// ([`mountain_rise_lead`](Self::mountain_rise_lead)) and eased
    /// ([`mountain_rise_power`](Self::mountain_rise_power)): the rise ends
    /// where a crest's elevation reaches its ceiling of one instead of 0.12
    /// short of it.
    pub mountain_rise: f64,
    /// How far under where the mountains stand alone their relief starts to
    /// rise, in normalised elevation (ticket 0004, step 4): 0.06, from 0.79,
    /// inside the blend, where the eased rise is still a hundredth of it and
    /// so no scarp (`tests/continuity.rs`).
    pub mountain_rise_lead: f64,
    /// The power the rise's smoothstep is raised to (ticket 0004, step 4):
    /// at one it is the smoothstep; at 2 the relief is low where it enters
    /// and comes in near the crest, so a flank steepens towards it.
    pub mountain_rise_power: usize,
    /// Metres a range adds to the ground at its crest line, over and above
    /// the elevation's share of it
    /// ([`Continent::range_m`](crate::Continent::range_m), ticket 0004,
    /// step 4). Until step 4 a range's height beyond
    /// the elevation's 756 m was the mountains' relief (a lift of 400 m and
    /// 1 500 of ridged noise), which came in over the 0.12 of elevation where
    /// they stood alone, a band a range's bell crossed in 250 to 750 m: the
    /// wall at every range's foot (the step's decomposition). Folded into a
    /// profile of the range's own, it rises over kilometres. 1 600 m: the
    /// city's upper terrace sees 6.06 degrees (5.85 at 1 500 m); 1 700 m
    /// flagged a fourth flank profile.
    pub range_height_m: f64,
    /// How far from its crest line a range's lift reaches, in metres (ticket
    /// 0004, step 4). The lift is read from the distance to the crest line
    /// (the first-order distance to the range field's zero contour), not
    /// from the field's value: the field's rate of change varies threefold
    /// along a range, and a profile in its value was a flank of 2 km in one
    /// place and a plateau of 5 km in another (the step's profiles showed
    /// both). 14 km with [`range_foot`](Self::range_foot) 3: the mountains
    /// 11.9 % of the land, the highlands 12.8 %.
    pub range_width_m: f64,
    /// The distance from the crest line within which the range's lift is
    /// rounded, metres: `sqrt(d^2 + c^2)` in place of the distance `d`, so
    /// the crest is a curve and not a roof line.
    pub range_round_m: f64,
    /// How far from its crest line a range's own metres reach, in metres:
    /// 10 km. At 8 km the lowland between a coast and a range lay level
    /// against the shore's ramp (a shelf flagged on six profiles); at 12 km
    /// its foot lifted the plains over their 400 m.
    pub range_height_width: f64,
    /// The power of a range's own profile across its line: 3, concave all
    /// the way to the crest, so the flank steepens towards it; at 2 the
    /// middle of the flank was steep enough to flag steps at the foot.
    pub range_height_power: usize,
    /// The distance from the crest line within which a range's own profile
    /// is rounded, metres.
    pub range_height_round: f64,
    /// The continent field from which a range's own metres stand whole;
    /// from 0.02 (the land's edge) by a smoothstep. 0.25: at 0.4 the
    /// lowland near the coast lay level in front of the ranges and more
    /// profiles flagged a shelf.
    pub range_height_coast: f64,
    /// How much of the other biomes' relief lies down up a range's flank:
    /// it is multiplied by `1 - flank_damping * flank`
    /// ([`Continent::flank`](crate::Continent::flank)), so the foothills
    /// roll at the foot and the flank above them rises as one slope, not as
    /// hills with shelves between them. 1: all of it at the crest line.
    pub flank_damping: f64,
    /// How much of a range's own slope the erosion filter follows and cuts
    /// by. The gullies cut as deep as the slope they follow; at the whole
    /// slope they cut 600 m into a steep flank in 250 m of it, a wall of
    /// the erosion's own (the step's decomposition, profile 4). 0.3.
    pub range_erosion_share: f64,
    /// The elevations over which a range's own metres come in: none under
    /// the first, all over the second. The lowland biomes are authored
    /// against the elevation, which the range's own metres are not in:
    /// without this the plains at a range's foot stood 90 m over the 400
    /// they are held to (`tests/coverage.rs`). 0.62 to 0.72, the plains'
    /// top and the highlands' lower third.
    pub range_root: (f64, f64),
    /// How much a range's mountain relief grows towards its crest line: it
    /// is multiplied by `1 + crest_relief_gain * flank^crest_relief_power`
    /// ([`Continent::flank`](crate::Continent::flank), zero at the foot of
    /// the range's own metres and one on the crest), so the crests are
    /// rock with an edge again while the foot, where the flank is nothing,
    /// keeps the soft rise step 4 gave it (ticket 0004, step 5). 0.7, with
    /// the power 1: 1 000 m of ridged relief on the crest line. More
    /// flagged the foot: the measure's foot is the lower third of the
    /// profile's whole rise, so a taller crest moves it up into steeper
    /// flank (1.5 at the power 3 flagged five of the ten profiles and seven
    /// of the nine opposite, 0.8 at the power 1 four of the opposite, where
    /// three are allowed), and the cols between serrated peaks read as
    /// shelves.
    pub crest_relief_gain: f64,
    /// See [`crest_relief_gain`](Self::crest_relief_gain): how late up the
    /// flank the growth comes in; 1 is in proportion to the flank. Higher
    /// powers put the whole growth into the top kilometre or two, where the
    /// measure's shelves are.
    pub crest_relief_power: usize,
    /// The share of a hard bed's run its cliff takes (ticket 0004, step 5):
    /// the bed rises its whole thickness by a smoothstep over this share of
    /// its bed coordinate and the rest is the bed's top, a bench. Where the
    /// beds lie level a riser on ground of slope `s` stands at about
    /// `s * (1 + strength * (1.5 / bed_riser - 1))`, and
    /// [`bed_steepening`](Self::bed_steepening) widens it where that is
    /// steeper.
    pub bed_riser: f64,
    /// How many times steeper than the bed coordinate (the ground's slope
    /// with the dip's) a hard bed's riser may stand at its steepest at full
    /// strength; a province of `strength` `k` stands it at the ground's
    /// slope plus `k (bed_steepening - 1)` times the bed coordinate's, so
    /// `1 + k (bed_steepening - 1)` times the ground's where the beds lie
    /// level, and more beside a steep dip (the fade bounds that).
    /// The riser takes more of its bed where [`bed_riser`](Self::bed_riser)
    /// would stand it steeper. A bound by construction, since the height
    /// may not depend on the ground's own slope (which jumps along every
    /// crease of a ridged or cellular relief, and would step the ground).
    pub bed_steepening: f64,
    /// The steepness of the bed coordinate (the ground's slope with the
    /// dip's) over which the beds fade out, rise over run: ground already a
    /// cliff needs no riser, and one there would stand more than the 8 m in
    /// a metre the ground is held to.
    pub bed_fade: (f64, f64),
    /// How far across the patches are in which a range's own ramp opens
    /// the beds' gate, metres (the review of step 5): a range's ramp is
    /// smooth and runs for tens of kilometres, and beds opened on all of it
    /// stood risers along a range foot's contours for 20 km without a
    /// break. A noise of this wavelength says how much of the ramp's slope
    /// the gate reads, so bands come and go along the foot; the relief's
    /// own flanks open it everywhere.
    pub bed_patch_wavelength_m: f64,
    /// The noise values over which a patch goes from none of the range's
    /// ramp read to all of it (see
    /// [`bed_patch_wavelength_m`](Self::bed_patch_wavelength_m)).
    pub bed_patch: (f64, f64),
    /// Spacing of the coarsest erosion gullies, in metres; each finer octave
    /// halves it. One value for the world, so gullies cross biome borders.
    pub erosion_wavelength_m: f64,
    /// How many octaves of gullies. Each branches off the one before.
    pub erosion_octaves: usize,
    /// The gradient at which gullies reach their full depth. Flatter ground
    /// is carved less, so summits and valley floors keep their shape.
    pub erosion_slope_reference: f64,
    /// How much each octave's crests and creases hold back the next one;
    /// higher leaves more fine gullies on the walls of coarse ones.
    pub erosion_detail: f64,
    /// How much wetter the bottom of a gully is than the ground around it.
    pub drainage_gain: f64,
    /// The province field's cell, metres: about how far one walks in one
    /// province (ticket 0004, step 3). At 24 km a straight line crosses one
    /// in 15.1 km on average (13.3 km median), about fifty minutes of play
    /// at a walk (5 m/s); over land, which is narrow, the mean is 8.5 km
    /// and the median 6.7. Seed 42 within 100 km of the origin, on the
    /// world after step 2b: its ranges hand ground to the range rocks and
    /// so cut provinces across; the cells alone give a mean of 20.1 km and
    /// a median of 21.6.
    pub province_cell_m: f64,
    /// How far a cell's point may sit from its cell's centre, as a share of
    /// the cell, at most 0.5: in the middle half, the 3 by 3 cells round a
    /// point hold its nearest point and every neighbour within the blend,
    /// so the field's search is exact. Less is a more regular lattice.
    pub province_jitter: f64,
    /// How far the border wiggle moves a border between two cells whose
    /// weights differ by one, at the noise's extreme, metres. 1.4 km bends
    /// borders enough that none reads as ruled at 5 to 10 km; the field's
    /// search stays exact while twice this plus the blend is under 0.22 of
    /// a cell (5.3 km at 24 km).
    pub province_wiggle_m: f64,
    /// The wavelength of the wiggle's one octave, metres: the scale a
    /// border bends at. 8 km, a third of a cell.
    pub province_wiggle_wavelength_m: f64,
    /// The full width of a border's blend between two provinces, metres.
    /// 1 km, as a biome border's: each province makes its own ground, and
    /// the blend is what keeps their meeting from being a seam.
    pub province_blend_m: f64,
    /// The range field ([`Continent::range`](crate::Continent::range)) from
    /// which a province starts to yield to the one its file names for
    /// ranges: a range's upper flank. The design chose 0.25 on the world
    /// before ticket 0004's step 2b, whose ranges rose over a smoothstep;
    /// since that step's bell (`range_foot`) the field spends kilometres of
    /// foot between 0.25 and 0.6 (2.5 km across at the median, 7.5 at the
    /// 90th percentile), which put 15.9 % of the land within 100 km in a
    /// blend against the design's 7 to 13. At 0.35 (step 3) the band is
    /// 1.7 km across at the median and 4.3 at the 90th percentile, 12.9 % of
    /// the land is in a blend, and on 400 random 20 km lines no weight
    /// changes by more than 0.0086 a metre; at 0.4 it was 0.0106, over the
    /// design's 0.01.
    pub province_range_lo: f64,
    /// The range field at which a province has yielded wholly. At 0.6 the
    /// crests (the field over 0.6) held no sandstone, basalt or badlands in
    /// the design's prototype: they are granite, folded rock and karst.
    pub province_range_hi: f64,
}

impl Default for WorldParams {
    fn default() -> Self {
        Self {
            continent_wavelength_m: 45_000.0,
            continent_octaves: 4,
            warp_wavelength_m: 18_000.0,
            warp_amplitude_m: 3_000.0,
            land_bias: -0.03,
            shore_width: 0.03,
            shore_step: 0.05,
            land_rise: 0.2,
            range_wavelength_m: 47_500.0,
            range_octaves: 2,
            range_persistence: 0.4,
            range_sharpness: 2.0,
            range_anisotropy: 2.5,
            range_floor: 0.0,
            range_crest: 1.0,
            range_foot: 3,
            rain_range_foot: 6,
            range_coast: 0.28,
            range_coast_power: 5,
            range_strength: 0.42,
            ocean_depth_m: 3_000.0,
            land_base_m: 900.0,
            equator_z: -56_000.0,
            pole_distance_m: 110_000.0,
            lapse_per_m: 1.0 / 4_000.0,
            temperature_noise: 0.12,
            temperature_wavelength_m: 12_000.0,
            moisture_wavelength_m: 25_000.0,
            moisture_stretch: 0.9,
            wind_turn_wavelength_m: 400_000.0,
            wind_turn: 0.35,
            rain_taps: 5,
            rain_tap_spacing_m: 3_600.0,
            rain_coarse_octaves: 2,
            rain_land_decay: 0.24,
            rain_orographic_rate: 0.9,
            rain_rise_scale_m: 200.0,
            rain_noise_share: 0.25,
            hydration_radius_m: 250.0,
            hydration_relief_scale_m: 40.0,
            hydration_hollow_gain: 0.25,
            hydration_slope_loss: 0.3,
            cover_dry: 0.3,
            cover_wet: 0.7,
            cover_wavelength_m: 600.0,
            cover_contrast: 1.6,
            blend_gap: 0.08,
            mountain_rise: 0.2,
            mountain_rise_lead: 0.06,
            mountain_rise_power: 2,
            range_height_m: 1_600.0,
            range_width_m: 14_000.0,
            range_round_m: 300.0,
            range_height_width: 10_000.0,
            range_height_power: 3,
            range_height_round: 400.0,
            range_height_coast: 0.25,
            flank_damping: 1.0,
            range_erosion_share: 0.3,
            range_root: (0.62, 0.72),
            crest_relief_gain: 0.7,
            crest_relief_power: 1,
            bed_riser: 0.2,
            bed_steepening: 5.0,
            bed_fade: (0.5, 1.0),
            bed_patch_wavelength_m: 2_500.0,
            bed_patch: (-0.55, -0.15),
            erosion_wavelength_m: 1_200.0,
            erosion_octaves: 5,
            erosion_slope_reference: 0.5,
            erosion_detail: 1.5,
            drainage_gain: 0.25,
            province_cell_m: 24_000.0,
            province_jitter: 0.5,
            province_wiggle_m: 1_400.0,
            province_wiggle_wavelength_m: 8_000.0,
            province_blend_m: 1_000.0,
            province_range_lo: 0.35,
            province_range_hi: 0.6,
        }
    }
}
