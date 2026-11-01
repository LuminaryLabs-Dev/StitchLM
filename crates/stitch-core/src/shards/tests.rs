use super::*;
fn fixture() -> (ShardDefinition, Vec<u8>, Vec<f32>) {
    let values: Vec<f32> = (0..16).map(|n| n as f32 / 8.).collect();
    let weights = float_bytes(&values);
    let input = vec![1.; 4];
    let spec = ProjectionSpec {
        id: "matrix".into(),
        request_id: "request".into(),
        model: crate::ModelIdentity {
            id: "synthetic-test-model".into(),
            revision: "1".repeat(40),
            weights_sha256: digest(&weights),
        },
        tensor_id: "test-weights".into(),
        rows: 4,
        columns: 4,
        packed: false,
        owner: "host".into(),
        devices: vec!["a".into(), "b".into()],
        memory_limit: 1_000_000,
        runtime_overhead: 0,
    };
    (
        projection_definition(&spec, &weights, &input).unwrap(),
        weights,
        input,
    )
}
fn packet(c: &Command, d: &ShardDefinition, value: f32) -> ResultPacket {
    let output = vec![value; d.computation.output_shape[0] as usize];
    ResultPacket {
        command_id: c.id.clone(),
        shard_id: c.shard_id.clone(),
        definition_hash: c.definition_hash.clone(),
        request_id: c.request_id.clone(),
        plan_version: c.plan_version,
        device_id: c.device_id.clone(),
        assignment_generation: c.assignment_generation,
        checksum: digest(&float_bytes(&output)),
        output,
        execution_ms: Some(1.),
        transferred_bytes: d.tensor.byte_length,
    }
}
fn profile(id: &str, ms: f64) -> DeviceCapabilities {
    DeviceCapabilities {
        id: id.into(),
        version: 1,
        backend: "cpu".into(),
        capabilities: vec!["f32".into(), "little_endian".into()],
        kernels: vec!["dense-f32-matvec/1".into()],
        available_memory: 1_000_000,
        runtime_overhead: 0,
        current_workload: 0,
        maximum_in_flight: 1,
        network_latency_ms: Some(0.),
        bandwidth_bytes_per_ms: Some(1_000_000.),
        queue_delay_ms: Some(0.),
        measured_at_ms: 100,
        measurements: vec![Measurement {
            operation: Operation::DenseMatVec,
            kernel: "dense-f32-matvec/1".into(),
            units: 16,
            execution_ms: ms,
            loading_ms: 0.,
            conversion_ms: 0.,
            result_return_ms: 0.,
            measured_at_ms: 100,
            samples: 1,
            uncertainty_ms: 0.,
            end_to_end_ms: Some(ms),
        }],
        thermal_state: None,
        battery_percent: None,
        background_execution: Some(true),
        online: true,
        heartbeat_ms: 100,
        allowed_sensitivities: vec![Sensitivity::Internal],
    }
}
#[test]
fn immutable_definition_conflicts_fail() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    let mut changed = d;
    changed.budget.cpu_percent = 5;
    assert!(b.define(changed).is_err());
}
#[test]
fn row_split_preserves_bytes_and_ranges() {
    let (d, w, _) = fixture();
    let parts = split_rows(&d, &w, 2).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1].storage.byte_range.start, 32);
    assert_eq!(parts[1].tensor.checksum, digest(&w[32..]));
    assert_eq!(parts[1].assembly.output_slice, Range { start: 2, end: 4 });
}
#[test]
fn corrupted_parent_bytes_rejected() {
    let (d, mut w, _) = fixture();
    w[0] ^= 1;
    assert!(split_rows(&d, &w, 2).is_err());
}
#[test]
fn oversized_parent_can_split_into_fitting_children() {
    let (mut d, w, _) = fixture();
    d.budget.memory_limit = 131200;
    assert!(!d.budget.fits());
    let children = split_rows(&d, &w, 4).unwrap();
    assert!(children.len() > 1);
    assert!(children.iter().all(|d| d.budget.fits()));
}
#[test]
fn stateful_splitting_requires_specific_contract() {
    let (mut d, w, _) = fixture();
    d.splitting.independent = false;
    assert!(split_rows(&d, &w, 2).is_err());
}
#[test]
fn priorities_dependencies_and_backpressure() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(2, 1).unwrap();
    b.define(d.clone()).unwrap();
    let mut child = d.clone();
    child.id = "dependent".into();
    child.operation_id = "dependent-op".into();
    child.inputs.prerequisites = vec![d.operation_id.clone()];
    b.define(child.clone()).unwrap();
    b.enqueue("first", &d.id, 1, 100, 100, None).unwrap();
    b.enqueue("second", &child.id, 9, 100, 100, None).unwrap();
    assert_eq!(b.ready(101), vec!["first"]);
    assert!(b.enqueue("overflow", &d.id, 0, 100, 100, None).is_err());
    let c = b.dispatch("first", "a", 101).unwrap();
    let mut store = ResourceStore::new(1000);
    b.accept(packet(&c, &d, 1.), 102, &mut store).unwrap();
    assert_eq!(b.ready(103), vec!["second"]);
}
#[test]
fn duplicates_are_idempotent_and_conflicts_rejected() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    b.enqueue("job", &d.id, 0, 100, 100, None).unwrap();
    let c = b.dispatch("job", "a", 101).unwrap();
    let mut store = ResourceStore::new(1000);
    let p = packet(&c, &d, 1.);
    assert!(b.accept(p.clone(), 102, &mut store).unwrap());
    assert!(!b.accept(p, 103, &mut store).unwrap());
    assert!(b.accept(packet(&c, &d, 2.), 104, &mut store).is_err());
}
#[test]
fn retry_invalidates_old_lease() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    b.enqueue("job", &d.id, 0, 100, 10, None).unwrap();
    let old = b.dispatch("job", "a", 101).unwrap();
    assert_eq!(b.expire(112), vec!["job"]);
    b.retry("job", 113, "timeout").unwrap();
    let new = b.dispatch("job", "b", 114).unwrap();
    let mut store = ResourceStore::new(1000);
    assert!(b.accept(packet(&old, &d, 1.), 115, &mut store).is_err());
    assert!(b.accept(packet(&new, &d, 1.), 115, &mut store).unwrap());
}
#[test]
fn cancellation_rejects_late_result() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    b.enqueue("job", &d.id, 0, 100, 100, None).unwrap();
    let c = b.dispatch("job", "a", 101).unwrap();
    b.cancel("job", "test").unwrap();
    assert!(b
        .accept(packet(&c, &d, 1.), 102, &mut ResourceStore::new(1000))
        .is_err());
    assert!(b.retry("job", 103, "retry").is_err());
}
#[test]
fn heartbeat_obeys_identity_and_absolute_deadline() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    b.enqueue("job", &d.id, 0, 100, 10, Some(120)).unwrap();
    let c = b.dispatch("job", "a", 101).unwrap();
    assert!(b
        .heartbeat("job", "b", c.assignment_generation, 105)
        .is_err());
    b.heartbeat("job", "a", c.assignment_generation, 110)
        .unwrap();
    assert_eq!(b.records["job"].command.lease_expires_ms, 120);
}
#[test]
fn scheduler_excludes_slow_incompatible_and_oversized_devices() {
    let (d, _, _) = fixture();
    let fast = profile("a", 1.);
    let slow = profile("b", 30.);
    assert_eq!(
        choose_device(
            &d,
            &[slow.clone(), fast.clone()],
            16,
            Some(10.),
            101,
            100,
            Objective::Latency
        )
        .unwrap()
        .unwrap()
        .device_id,
        "a"
    );
    assert!(
        choose_device(&d, &[slow], 16, Some(10.), 101, 100, Objective::Latency)
            .unwrap()
            .is_none()
    );
    let mut low = fast;
    low.available_memory = 1;
    assert!(
        choose_device(&d, &[low], 16, None, 101, 100, Objective::Capacity)
            .unwrap()
            .is_none()
    );
}
#[test]
fn unknown_cost_is_not_claimed_as_speed_gain() {
    let (d, _, _) = fixture();
    let mut p = profile("a", 1.);
    p.measurements.clear();
    assert!(choose_device(
        &d,
        &[p.clone()],
        16,
        Some(10.),
        101,
        100,
        Objective::Latency
    )
    .unwrap()
    .is_none());
    let v = choose_device(&d, &[p], 16, None, 101, 100, Objective::Capacity)
        .unwrap()
        .unwrap();
    assert!(!v.measured);
    assert!(v.expected_saved_ms.is_none());
}
fn part(id: &str, start: u64, end: u64, values: Vec<f32>) -> SliceResult {
    SliceResult {
        contributor: id.into(),
        range: Range { start, end },
        checksum: digest(&float_bytes(&values)),
        values,
    }
}
#[test]
fn concatenation_rejects_gaps_overlap_missing_and_corruption() {
    let a = part("a", 0, 2, vec![1., 2.]);
    let b = part("b", 2, 4, vec![3., 4.]);
    let ids = vec!["a".into(), "b".into()];
    assert_eq!(
        concatenate(&[b.clone(), a.clone()], &ids, Range { start: 0, end: 4 }).unwrap(),
        vec![1., 2., 3., 4.]
    );
    let mut bad = b;
    bad.range.start = 1;
    assert!(concatenate(&[a.clone(), bad], &ids, Range { start: 0, end: 4 }).is_err());
    assert!(concatenate(&[a], &ids, Range { start: 0, end: 4 }).is_err());
}
#[test]
fn reduction_sums_in_stable_range_order() {
    let parts = vec![part("b", 2, 4, vec![3., 4.]), part("a", 0, 2, vec![1., 2.])];
    assert_eq!(
        sum_reduction(
            &parts,
            &["a".into(), "b".into()],
            Range { start: 0, end: 4 },
            2
        )
        .unwrap(),
        vec![4., 6.]
    );
}
#[test]
fn attention_merge_matches_unsplit_softmax_even_large_logits() {
    let logits = vec![1000., 1001., 998., 1002.];
    let values = vec![vec![1., 2.], vec![3., 4.], vec![-1., 6.], vec![2., 8.]];
    let whole = attention_partial("whole", 0, &logits, &values).unwrap();
    let expected =
        merge_attention(&[whole], &["whole".into()], Range { start: 0, end: 4 }).unwrap();
    let a = attention_partial("a", 0, &logits[..2], &values[..2]).unwrap();
    let b = attention_partial("b", 2, &logits[2..], &values[2..]).unwrap();
    let output = merge_attention(
        &[b, a],
        &["a".into(), "b".into()],
        Range { start: 0, end: 4 },
    )
    .unwrap();
    for (a, b) in output.iter().zip(expected) {
        assert!((a - b).abs() < 1e-6);
    }
}
#[test]
fn resource_hash_version_and_permissions_enforced() {
    let r = ResourceRef {
        id: "resource".into(),
        version: 1,
        checksum: Some(digest(&[1, 2])),
        access: Access::ReadOnly,
        location: "memory".into(),
        available: true,
    };
    let mut store = ResourceStore::new(2);
    store
        .insert(ResourceEntry {
            reference: r.clone(),
            owner: "host".into(),
            allowed_devices: vec!["a".into()],
            sensitivity: Sensitivity::Private,
            bytes: vec![1, 2],
        })
        .unwrap();
    assert_eq!(store.get(&r, "a").unwrap(), [1, 2]);
    assert!(store.get(&r, "b").is_err());
    let mut stale = r;
    stale.version = 2;
    assert!(store.get(&stale, "a").is_err());
}
#[test]
fn checkpoint_preserves_completed_outputs_and_rejects_tampering() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    b.enqueue("job", &d.id, 0, 100, 100, None).unwrap();
    let c = b.dispatch("job", "a", 101).unwrap();
    let mut store = ResourceStore::new(1000);
    b.accept(packet(&c, &d, 1.), 102, &mut store).unwrap();
    let cp = b
        .checkpoint("request", &d.weight_checksum, 1, 0, &store)
        .unwrap();
    let restored = CommandBuffer::restore(&cp, &store, 8, 2).unwrap();
    assert_eq!(restored.records["job"].status, Status::Completed);
    let missing = CommandBuffer::restore(&cp, &ResourceStore::new(1000), 8, 2).unwrap();
    assert_eq!(missing.records["job"].status, Status::ReplayRequired);
    let mut altered = cp;
    altered.token_position = 9;
    assert!(altered.verify().is_err());
}
#[test]
fn state_commits_require_owner_and_expected_version() {
    let mut b = CommandBuffer::new(8, 2).unwrap();
    let state = StateVersion {
        id: "state".into(),
        request_id: "request".into(),
        model_checksum: "a".repeat(64),
        version: 1,
        owner: "a".into(),
        committed_position: 1,
        checksum: "b".repeat(64),
        location: "worker:a".into(),
        kind: "recurrent".into(),
        replay_required: true,
    };
    b.commit_state(state.clone(), 0, "a").unwrap();
    let mut next = state;
    next.version = 2;
    next.committed_position = 2;
    assert!(b.commit_state(next.clone(), 1, "b").is_err());
    assert!(b.commit_state(next.clone(), 0, "a").is_err());
    b.commit_state(next, 1, "a").unwrap();
}
#[test]
fn column_split_repackages_correct_inputs_and_weights() {
    let (d, w, input) = fixture();
    let parts = split_columns(&d, &w, &input, 2).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].input, vec![1., 1.]);
    assert_eq!(parts[1].source_range, Range { start: 2, end: 4 });
    let values: Vec<f32> = parts[1]
        .weights
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    assert_eq!(
        values,
        vec![0.25, 0.375, 0.75, 0.875, 1.25, 1.375, 1.75, 1.875]
    );
    assert_eq!(parts[1].definition.assembly.operation, Merge::Sum);
}

#[test]
fn inflight_checkpoint_replans_with_a_new_generation() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    b.enqueue("job", &d.id, 0, 100, 100, None).unwrap();
    let old = b.dispatch("job", "a", 101).unwrap();
    let store = ResourceStore::new(1000);
    let cp = b
        .checkpoint("request", &d.weight_checksum, 1, 0, &store)
        .unwrap();
    assert_eq!(cp.execution_position, 0);
    let mut restored = CommandBuffer::restore(&cp, &store, 8, 2).unwrap();
    let new = restored.dispatch("job", "b", 102).unwrap();
    assert!(new.assignment_generation > old.assignment_generation);
    let mut resources = ResourceStore::new(1000);
    assert!(restored
        .accept(packet(&old, &d, 1.), 103, &mut resources)
        .is_err());
    assert!(restored
        .accept(packet(&new, &d, 1.), 103, &mut resources)
        .unwrap());
}
#[test]
fn malformed_matrix_rank_is_rejected_without_panic() {
    let (mut d, w, input) = fixture();
    d.tensor.shape = vec![16];
    d.tensor.axes = vec!["rows".into()];
    d.tensor.strides = vec![1];
    d.tensor.slices.remove("columns");
    d.splitting.axes = vec!["rows".into()];
    assert!(split_rows(&d, &w, 1).is_err());
    assert!(split_columns(&d, &w, &input, 1).is_err());
}
#[test]
fn mutable_state_migration_requires_retained_bytes() {
    let mut b = CommandBuffer::new(8, 2).unwrap();
    let bytes = vec![1, 2, 3, 4];
    let hash = digest(&bytes);
    let state = StateVersion {
        id: "cache".into(),
        request_id: "r".into(),
        model_checksum: "a".repeat(64),
        version: 1,
        owner: "a".into(),
        committed_position: 2,
        checksum: hash.clone(),
        location: "memory".into(),
        kind: "recurrent".into(),
        replay_required: false,
    };
    b.commit_state(state, 0, "a").unwrap();
    let mut store = ResourceStore::new(100);
    assert!(b
        .migrate_state("cache", 1, "a", "b", &["b".into()], &store)
        .is_err());
    store
        .insert(ResourceEntry {
            reference: ResourceRef {
                id: "cache".into(),
                version: 1,
                checksum: Some(hash),
                access: Access::Exclusive,
                location: "memory".into(),
                available: true,
            },
            owner: "a".into(),
            allowed_devices: vec!["b".into()],
            sensitivity: Sensitivity::Private,
            bytes,
        })
        .unwrap();
    b.migrate_state("cache", 1, "a", "b", &["b".into()], &store)
        .unwrap();
    assert_eq!(b.states["cache"].owner, "b");
    assert_eq!(b.states["cache"].version, 2);
    assert!(b.states["cache"].replay_required);
}

#[test]
fn checkpoint_roundtrip_preserves_measured_float_costs() {
    let (d, _, _) = fixture();
    let mut buffer = CommandBuffer::new(8, 2).unwrap();
    buffer.define(d.clone()).unwrap();
    buffer.enqueue("job", &d.id, 0, 100, 100, None).unwrap();
    buffer.dispatch("job", "a", 101).unwrap();
    buffer.records.get_mut("job").unwrap().placement = Some(Placement {
        device_id: "a".into(),
        predicted_ms: Some(117.7),
        expected_saved_ms: Some(-109.13356394117648),
        measured: true,
        reason: "observed round-trip regression".into(),
    });
    let cp = buffer
        .checkpoint(
            "request",
            &d.weight_checksum,
            1,
            0,
            &ResourceStore::new(1000),
        )
        .unwrap();
    let wire = serde_json::to_string(&cp).unwrap();
    let decoded: Checkpoint = serde_json::from_str(&wire).unwrap();
    decoded.verify().unwrap();
    assert_eq!(cp.checksum, decoded.checksum);
}
#[test]
fn empty_attention_segments_fail_explicitly() {
    assert!(attention_partial("empty", 0, &[], &[]).is_err());
    assert!(merge_attention(&[], &[], Range { start: 0, end: 1 }).is_err());
}

#[test]
fn dependencies_do_not_cross_request_boundaries() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    let mut other = d.clone();
    other.id = "other-producer".into();
    other.request_id = "other-request".into();
    b.define(other.clone()).unwrap();
    let mut consumer = other.clone();
    consumer.id = "other-consumer".into();
    consumer.operation_id = "consumer".into();
    consumer.inputs.prerequisites = vec![other.operation_id.clone()];
    b.define(consumer.clone()).unwrap();
    b.enqueue("a", &d.id, 0, 100, 100, None).unwrap();
    b.enqueue("b", &other.id, 0, 100, 100, None).unwrap();
    b.enqueue("c", &consumer.id, 0, 100, 100, None).unwrap();
    b.validate_graph().unwrap();
    let command = b.dispatch("a", "a", 101).unwrap();
    b.accept(packet(&command, &d, 1.), 102, &mut ResourceStore::new(1000))
        .unwrap();
    assert!(!b.ready(103).contains(&"c".to_string()));
}
#[test]
fn operation_dependency_waits_for_all_producer_shards() {
    let (d, _, _) = fixture();
    let mut b = CommandBuffer::new(8, 2).unwrap();
    b.define(d.clone()).unwrap();
    let mut sibling = d.clone();
    sibling.id = "sibling".into();
    b.define(sibling.clone()).unwrap();
    let mut consumer = d.clone();
    consumer.id = "consumer".into();
    consumer.operation_id = "consumer-op".into();
    consumer.inputs.prerequisites = vec![d.operation_id.clone()];
    b.define(consumer.clone()).unwrap();
    for (id, def) in [("a", &d), ("b", &sibling), ("c", &consumer)] {
        b.enqueue(id, &def.id, 0, 100, 100, None).unwrap();
    }
    let command = b.dispatch("a", "a", 101).unwrap();
    b.accept(packet(&command, &d, 1.), 102, &mut ResourceStore::new(1000))
        .unwrap();
    assert!(!b.ready(103).contains(&"c".to_string()));
}
#[test]
fn cyclic_dependency_graph_is_rejected() {
    let (mut a, _, _) = fixture();
    let mut b = a.clone();
    a.operation_id = "a".into();
    a.inputs.prerequisites = vec!["b".into()];
    b.id = "b-def".into();
    b.operation_id = "b".into();
    b.inputs.prerequisites = vec!["a".into()];
    let mut graph = CommandBuffer::new(8, 2).unwrap();
    graph.define(a.clone()).unwrap();
    graph.define(b.clone()).unwrap();
    graph.enqueue("a-job", &a.id, 0, 100, 100, None).unwrap();
    graph.enqueue("b-job", &b.id, 0, 100, 100, None).unwrap();
    assert!(graph.validate_graph().is_err());
}
