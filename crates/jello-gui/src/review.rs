use std::sync::Arc;

use jello::{
    Diagnostic, RepairDecision, RepairDecisionSetId, RepairEvaluation, RepairGroupId, RepairPlan,
    RepairSelection,
};

#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct ReviewState {
    plan: Arc<RepairPlan>,
    selection: RepairSelection,
    evaluation: RepairEvaluation,
    selected_group: Option<RepairGroupId>,
    evaluation_pending: bool,
    selection_version: u64,
}

#[allow(dead_code)]
impl ReviewState {
    pub(crate) fn new(
        plan: Arc<RepairPlan>,
        selection: RepairSelection,
        evaluation: RepairEvaluation,
    ) -> Self {
        Self {
            plan,
            selection,
            evaluation,
            selected_group: None,
            evaluation_pending: false,
            selection_version: 0,
        }
    }

    pub(crate) fn plan(&self) -> &Arc<RepairPlan> {
        &self.plan
    }

    pub(crate) fn selection(&self) -> &RepairSelection {
        &self.selection
    }

    pub(crate) fn evaluation(&self) -> &RepairEvaluation {
        &self.evaluation
    }

    pub(crate) fn replace_evaluation(&mut self, evaluation: RepairEvaluation) {
        self.evaluation = evaluation;
        self.evaluation_pending = false;
    }

    pub(crate) fn decide(
        &mut self,
        decision_set: RepairDecisionSetId,
        decision: RepairDecision,
    ) -> bool {
        if !self.selection.set(decision_set, decision) {
            return false;
        }
        self.selection_changed();
        true
    }

    pub(crate) fn set_all(&mut self, decision: RepairDecision) {
        self.selection.set_all(decision);
        self.selection_changed();
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.selection
            .decisions()
            .iter()
            .filter(|decision| **decision == RepairDecision::Pending)
            .count()
    }

    pub(crate) fn selected_group(&self) -> Option<RepairGroupId> {
        self.selected_group
    }

    pub(crate) fn set_selected_group(&mut self, group: Option<RepairGroupId>) {
        self.selected_group = group;
    }

    pub(crate) fn selection_version(&self) -> u64 {
        self.selection_version
    }

    pub(crate) fn evaluation_pending(&self) -> bool {
        self.evaluation_pending
    }

    pub(crate) fn mark_evaluation_pending(&mut self) {
        self.evaluation_pending = true;
    }

    pub(crate) fn invalid_diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        match &self.evaluation {
            RepairEvaluation::Invalid { diagnostics, .. } => diagnostics.iter(),
            RepairEvaluation::Preview(_) | RepairEvaluation::Ready(_) => [].iter(),
        }
    }

    pub(crate) fn can_save(&self) -> bool {
        matches!(self.evaluation, RepairEvaluation::Ready(_))
            && self.pending_count() == 0
            && !self.evaluation_pending
    }

    fn selection_changed(&mut self) {
        self.selection_version = self.selection_version.saturating_add(1);
        self.mark_evaluation_pending();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jello::RepairDecision;

    use super::ReviewState;

    #[test]
    fn pending_review_blocks_save_until_every_set_is_accepted() {
        let plan = Arc::new(jello::plan_repair_json5("{name:'Ada'}").unwrap());
        let selection = plan.default_selection();
        let evaluation = plan.evaluate(&selection);
        let mut state = ReviewState::new(plan, selection, evaluation);
        assert!(!state.can_save());
        state.set_all(RepairDecision::Accepted);
        state.replace_evaluation(state.plan().evaluate(state.selection()));
        assert!(state.can_save());
    }

    #[test]
    fn rejected_required_repair_keeps_review_but_blocks_save() {
        let plan = Arc::new(jello::plan_repair_json5("{name:'Ada'}").unwrap());
        let selection = plan.default_selection();
        let evaluation = plan.evaluate(&selection);
        let mut state = ReviewState::new(plan, selection, evaluation);
        state.set_all(RepairDecision::Rejected);
        state.replace_evaluation(state.plan().evaluate(state.selection()));
        assert!(!state.can_save());
        assert!(state.invalid_diagnostics().next().is_some());
    }
}
