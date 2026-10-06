//! Original scalar JS dependency-connected domain on a proven common owner clock.
//! Fitted dependency tracks are never fed back into authored script evaluation.
use super::*;

struct Node<'a> {
    entry: &'a AnimationGraphEntry,
    inputs: Vec<usize>,
    source_index: usize,
}
pub(super) struct Program<'a> {
    nodes: Vec<Node<'a>>,
    output: usize,
    order: Vec<usize>,
}
impl<'a> Program<'a> {
    pub(super) fn new(
        entry: &AnimationGraphEntry,
        entries: &'a [AnimationGraphEntry],
        owner: Owner,
        owner_of: &impl Fn(&AnimationGraphEntry) -> Option<Owner>,
    ) -> Result<Self, BakeError> {
        // A sibling consumer can modify globals between a producer and this
        // consumer. Playback runs the whole dependency-connected domain, not
        // just this target's upstream closure. Include downstream edges too.
        let mut domain = BTreeSet::from([entry.target.clone()]);
        loop {
            let previous_len = domain.len();
            for candidate in entries {
                if domain.contains(&candidate.target)
                    || candidate
                        .dependencies
                        .iter()
                        .any(|target| domain.contains(target))
                {
                    domain.insert(candidate.target.clone());
                    domain.extend(candidate.dependencies.iter().cloned());
                }
            }
            if domain.len() == previous_len {
                break;
            }
        }
        let mut program = Self {
            nodes: Vec::new(),
            output: 0,
            order: Vec::new(),
        };
        let mut visiting = BTreeSet::new();
        for target in &domain {
            program.add(target, entries, owner, owner_of, &mut visiting)?;
        }
        program.output = program
            .nodes
            .iter()
            .position(|node| node.entry.target == entry.target)
            .expect("domain includes the consumer");
        // Same stable-ready tie-break as the graph oracle: serialized entry
        // order, not authored dependency-slot order or depth-first traversal.
        let mut emitted = vec![false; program.nodes.len()];
        while let Some(index) = (0..program.nodes.len())
            .filter(|&index| {
                !emitted[index]
                    && program.nodes[index]
                        .inputs
                        .iter()
                        .all(|&input| emitted[input])
            })
            .min_by_key(|&index| program.nodes[index].source_index)
        {
            emitted[index] = true;
            program.order.push(index);
        }
        debug_assert_eq!(program.order.len(), program.nodes.len());
        Ok(program)
    }
    fn add(
        &mut self,
        target: &PropertyTarget,
        entries: &'a [AnimationGraphEntry],
        owner: Owner,
        owner_of: &impl Fn(&AnimationGraphEntry) -> Option<Owner>,
        visiting: &mut BTreeSet<PropertyTarget>,
    ) -> Result<usize, BakeError> {
        if !visiting.insert(target.clone()) {
            return Err(BakeError::Unsupported("cyclic script dependency"));
        }
        if let Some(index) = self
            .nodes
            .iter()
            .position(|node| &node.entry.target == target)
        {
            visiting.remove(target);
            return Ok(index);
        }
        let (source_index, entry) = entries
            .iter()
            .enumerate()
            .find(|(_, entry)| &entry.target == target)
            .ok_or(BakeError::Unsupported(
                "dependency has no authored scalar animator",
            ))?;
        validate_scalar_target(&entry.target)?;
        let dependency_owner =
            owner_of(entry).ok_or(BakeError::Unsupported("dependency owner is not available"))?;
        if dependency_owner.unsupported_clock
            || owner.clock_id != dependency_owner.clock_id
            || owner.start_ms != dependency_owner.start_ms
            || owner.duration_ms != dependency_owner.duration_ms
        {
            return Err(BakeError::Unsupported(
                "dependency owner clocks are not proven equivalent",
            ));
        }
        if !entry.layer_refs.is_empty()
            || !matches!(
                entry.animator.data(),
                AnimatorData::JsScript {
                    code: None,
                    layer_time_js_code: Some(_)
                }
            )
        {
            return Err(BakeError::Unsupported(
                "dependency is not an independent layer-time scalar script",
            ));
        }
        let mut inputs = Vec::new();
        for target in &entry.dependencies {
            inputs.push(self.add(target, entries, owner, owner_of, visiting)?);
        }
        visiting.remove(target);
        let index = self.nodes.len();
        self.nodes.push(Node {
            entry,
            inputs,
            source_index,
        });
        Ok(index)
    }
    pub(super) fn evaluate(
        &self,
        runtime: &mut ScriptRuntime,
        code: &str,
        seed: u64,
        time: u64,
        budget: &mut Budget,
    ) -> Result<f64, BakeError> {
        if self.nodes.len() == 1 {
            return super::evaluate(runtime, code, seed, time, budget);
        }
        // The authored graph shares one VM across the complete domain. Capture
        // the consumer at its graph position, not after its downstream scripts.
        // Fresh validation supplies one new shared VM.
        let mut values = vec![None; self.nodes.len()];
        for &index in &self.order {
            let node = &self.nodes[index];
            let AnimatorData::JsScript {
                layer_time_js_code: Some(node_code),
                ..
            } = node.entry.animator.data()
            else {
                unreachable!("dependency domain admits only layer-time scripts");
            };
            let inputs = node
                .inputs
                .iter()
                .map(|&index| {
                    values[index].expect("topological order evaluates every producer first")
                })
                .collect::<Vec<_>>();
            // The caller supplies this consumer's original source and seed;
            // never apply either to a sibling node in the domain.
            let (node_code, node_seed) = if index == self.output {
                (code, seed)
            } else {
                (
                    node_code.as_str(),
                    seed::prefix(
                        node.entry
                            .random_seed_target
                            .as_ref()
                            .unwrap_or(&node.entry.target),
                    ),
                )
            };
            values[index] = Some(call(runtime, node_code, node_seed, time, budget, &inputs)?);
        }
        Ok(values[self.output].expect("domain order evaluates the consumer"))
    }
}
fn call(
    runtime: &mut ScriptRuntime,
    code: &str,
    seed: u64,
    time: u64,
    budget: &mut Budget,
    values: &[f64],
) -> Result<f64, BakeError> {
    budget.sample()?;
    let input = input::build(runtime.context_mut(), time, random_seed(seed, time));
    input::set_dependencies(runtime.context_mut(), &input, values)?;
    let refs = input::empty_object(runtime.context_mut());
    let metadata = input::empty_object(runtime.context_mut());
    install_reference_tables(&input, refs, metadata, runtime.context_mut())?;
    runtime
        .call(code, input)?
        .as_number()
        .filter(|value| value.is_finite())
        .ok_or(BakeError::NonScalar(time))
}
