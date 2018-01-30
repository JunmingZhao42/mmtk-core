use ::vm::ActivePlan;
use ::plan::{Plan, SelectedPlan};
use ::util::Address;
use super::entrypoint::*;
use super::scheduling::VMScheduling;

pub struct VMActivePlan<> {}

impl<'a> ActivePlan<'a> for VMActivePlan {
    fn global() -> &'static SelectedPlan<'static> {
        &::plan::selected_plan::PLAN
    }

    fn collector(thread_id: usize) -> &'a mut <SelectedPlan<'a> as Plan>::CollectorT {
        unsafe {
            let thread = super::scheduling::VMScheduling::thread_from_id(thread_id);
            let system_thread = Address::from_usize(
                (thread + SYSTEM_THREAD_FIELD_OFFSET).load::<usize>());
            let cc = &mut *((system_thread + WORKER_INSTANCE_FIELD_OFFSET)
                .load::<*mut <SelectedPlan as Plan>::CollectorT>());

            cc
        }
    }

    fn is_mutator(thread_id: usize) -> bool {
        unsafe {
            let thread = VMScheduling::thread_from_id(thread_id);
            !(thread + IS_COLLECTOR_FIELD_OFFSET).load::<bool>()
        }
    }

    fn mutator(thread_id: usize) -> &'a mut <SelectedPlan<'a> as Plan>::MutatorT {
        unimplemented!()
    }

    fn collector_count(thread_id: usize) -> usize {
        unimplemented!()
    }

    fn reset_mutator_iterator(thread_id: usize) {
        unimplemented!()
    }

    fn get_next_mutator() -> Option<&'a mut <SelectedPlan<'a> as Plan>::MutatorT> {
        unimplemented!()
    }
}