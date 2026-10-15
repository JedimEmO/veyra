//! Baked weapon paths keep hit volumes aligned with the visible skinned asset.

use bevy::ecs::prelude::Resource;
use bevy::math::Vec3;
use serde::Deserialize;

use crate::combat::Action;

/// Endpoints are in character-local metres, with the origin at the feet.
#[derive(Debug, Clone, Deserialize)]
pub struct BladeSample {
    /// Normalised animation time, independent of an action's duration.
    pub u: f32,
    /// Hand socket from the fitted skeleton.
    pub hand: [f32; 3],
    /// End of the rendered blade along the socket's local axis.
    pub tip: [f32; 3],
}

/// Generated from the same bundled GLB that the client loads.
#[derive(Resource, Debug, Clone, Deserialize)]
pub struct MeleeCurves {
    /// Opening attack.
    pub light: Vec<BladeSample>,
    /// Return attack.
    pub followup: Vec<BladeSample>,
    /// Posture-breaking attack.
    pub heavy: Vec<BladeSample>,
}

impl MeleeCurves {
    /// Reject invalid data before a headless server could use a different path.
    ///
    /// # Errors
    /// Returns the malformed RON or the first invalid track.
    pub fn parse(text: &str) -> Result<Self, String> {
        let curves: Self = ron::from_str(text).map_err(|error| error.to_string())?;
        for track in [&curves.light, &curves.followup, &curves.heavy] {
            if track.len() < 2
                || track.first().is_none_or(|s| s.u != 0.)
                || track.last().is_none_or(|s| (s.u - 1.).abs() > 1e-6)
                || track.windows(2).any(|w| w[1].u <= w[0].u)
                || track.iter().any(|s| {
                    !s.u.is_finite()
                        || !Vec3::from(s.hand).is_finite()
                        || !Vec3::from(s.tip).is_finite()
                })
            {
                return Err("invalid combat blade track: expected finite, strictly ordered samples from 0 to 1".into());
            }
        }
        Ok(curves)
    }

    /// Linear segments match the exported sampling contract and are swept by physics.
    /// `duration` is the length the clip is played over (the class's retiming
    /// of the table's row, `Combatant::clip_duration`), and `reach` the
    /// factor on how far along and beyond the baked blade a point lies; a
    /// duration of the table's and a reach of one are the baked sweep.
    #[must_use]
    pub fn point(
        &self,
        action: Action,
        time: f32,
        duration: f32,
        reach: f32,
        along_blade: f32,
    ) -> Vec3 {
        let track = match action {
            Action::Followup => &self.followup,
            Action::Heavy => &self.heavy,
            _ => &self.light,
        };
        let u = (time / duration).clamp(0., 1.);
        let upper = track.partition_point(|s| s.u < u).clamp(1, track.len() - 1);
        let (a, b) = (&track[upper - 1], &track[upper]);
        let t = ((u - a.u) / (b.u - a.u)).clamp(0., 1.);
        let hand = Vec3::from(a.hand).lerp(Vec3::from(b.hand), t);
        let tip = Vec3::from(a.tip).lerp(Vec3::from(b.tip), t);
        hand.lerp(tip, along_blade * reach)
    }
}
