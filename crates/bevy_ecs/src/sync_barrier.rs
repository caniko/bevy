//! Scoped synchronization barriers for deterministic mutation pipelines.

use core::marker::PhantomData;

use crate::world::World;

/// Flushes pending writes for a declared component set.
///
/// The prototype implementation uses [`World::flush`] globally. The public
/// contract is intentionally phrased in terms of `S`: after the barrier runs,
/// pending writes to components in `S` are settled. A future implementation can
/// replace the global flush with per-set dispatch without changing downstream
/// schedules.
pub struct SyncBarrier<S> {
    marker: PhantomData<fn() -> S>,
}

impl<S> SyncBarrier<S> {
    /// Exclusive system suitable for schedule insertion.
    pub fn system(world: &mut World) {
        world.flush();
    }
}

impl<S> Default for SyncBarrier<S> {
    fn default() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        component::Component, schedule::Schedule, spawn::Spawn, sync_barrier::SyncBarrier,
        system::Commands, world::World,
    };

    #[derive(Component)]
    struct Marker;

    struct TestSet;

    #[test]
    fn barrier_flushes_deferred_commands() {
        fn spawn(mut commands: Commands) {
            commands.spawn(Spawn(Marker));
        }

        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((spawn, SyncBarrier::<TestSet>::system).chain());
        schedule.run(&mut world);

        assert_eq!(world.query::<&Marker>().iter(&world).count(), 1);
    }
}
