use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchStatus {
    pub batch: ResolvedControlBatch,
    pub stage: ReceiptStage,
    pub outcomes: Vec<Receipt>,
}

impl Store {
    pub fn control_status(&self, id: &ControlId) -> anyhow::Result<BatchStatus> {
        self.change(|s| {
            let batch = s
                .controls
                .batches
                .get(id)
                .context("unknown control")?
                .clone();
            let outcomes: Vec<_> = batch
                .members
                .iter()
                .filter_map(|member| {
                    s.controls
                        .receipts
                        .iter()
                        .rev()
                        .find(|r| r.correlation == member.id.as_str())
                        .cloned()
                })
                .collect();
            let stage = if let Some(first) = outcomes.first() {
                if outcomes.iter().all(|r| r.stage == first.stage) {
                    first.stage.clone()
                } else {
                    ReceiptStage::Partial
                }
            } else {
                ReceiptStage::Applied
            };
            Ok(BatchStatus {
                batch,
                stage,
                outcomes,
            })
        })
    }
}
