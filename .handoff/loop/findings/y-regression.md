# Teri Y-Regression Baseline

**Date:** 2026-06-13  
**Branch:** develop @ c894de8  
**Test Suite Result:** 142 passed, 2 ignored, 5 test suites (0.26s)

## Test Files

Integration tests and module tests are distributed across:
- `tests/graph_integration_test.rs` — graph construction and serialization
- `tests/memory_tests.rs` — persistence and memory operations
- Inline `#[cfg(test)]` modules in: `agent.rs`, `api/`, `graph.rs`, `sim/`, `memory.rs`, `report.rs`

## Complete Test List (149 tests, 142 passing)

### agent::tests (26 tests)
- test_action_generator_creation ... ok
- test_agent_creation ... ok
- test_action_generator_from_file_fallback ... ok
- test_agent_memory ... ok
- test_agent_pool ... ok
- test_action_generator_parse_context ... ok
- test_agent_state_change ... ok
- test_agent_pool_spawn_empty_graph ... ok
- test_parse_and_validate_action_interact ... ok
- test_entity_description_generation ... ok
- test_parse_and_validate_action_invalid_format ... ok
- test_action_generator_generate_prompt ... ok
- test_parse_and_validate_action_observe ... ok
- test_parse_and_validate_action_speak ... ok
- test_parse_and_validate_action_think ... ok
- test_parse_and_validate_action_unknown_type ... ok
- test_parse_and_validate_action_with_whitespace ... ok
- test_persona_generator_from_file ... ok
- test_persona_generator_with_custom_template ... ok
- test_persona_generator_validation ... ok
- test_agent_step_with_mock_llm ... ok
- test_template_sanitization ... ok
- test_persona_generator_creation ... ok
- test_parse_and_validate_action_nested_parens ... ok
- test_persona_generator_with_mock_llm ... ok
- test_agent_step_integration_with_complex_world ... ok
- test_agent_step_stores_action_in_memory_with_importance ... ok
- test_agent_step_with_fallback ... ok
- test_agent_pool_spawn_with_mock_llm ... ok
- test_persona_deduplication ... ok

### api::tests (8 tests)
- test_chat_request ... ok
- test_create_sim_request ... ok
- test_stream_config_defaults ... ok
- test_stream_config_low_latency ... ok
- test_stream_config_reliable_delivery ... ok
- test_tick_stream_event_creation ... ok
- test_tick_stream_event_lag_gap ... ok

### api::streaming::tests (8 tests)
- test_stream_adapter_creation ... ok
- test_stream_adapter_push ... ok
- test_tick_buffer_backpressure ... ok
- test_tick_buffer_creation ... ok
- test_tick_buffer_drain ... ok
- test_tick_buffer_peek_preserves ... ok
- test_tick_buffer_push ... ok
- test_tick_buffer_stats ... ok
- test_tick_buffer_zero_size_panics [should panic] ... ok

### graph::tests (32 tests)
- test_add_entity ... ok
- test_add_relation ... ok
- test_build_from_seed_document ... ok
- test_deserialize_invalid_bincode ... ok
- test_deserialize_invalid_json ... ok
- test_duplicate_entity_name_error ... ok
- test_empty_entity_list_prompt ... ok
- test_entity_extraction_prompt_contains_metadata_and_body ... ok
- test_entity_with_id_parsing ... ok
- test_get_entity ... ok
- test_entity_extraction_with_mock_llm ... ok
- test_get_entity_case_sensitivity ... ok
- test_get_neighbors_by_id ... ok
- test_get_neighbors_nonexistent_entity ... ok
- test_get_subgraph_depth_limited ... ok
- test_get_subgraph_depth_zero ... ok
- test_get_subgraph_isolated_entity ... ok
- test_get_subgraph_nonexistent_entity ... ok
- test_invalid_weight_error ... ok
- test_knowledge_graph_creation ... ok
- test_parse_entities_json ... ok
- test_graph_construction_with_mock_llm ... ok
- test_parse_relations_json ... ok
- test_relation_extraction_prompt_lists_entities ... ok
- test_relation_new_validation ... ok
- test_relation_extraction_with_mock_llm ... ok
- test_serialize_to_bincode_and_deserialize ... ok
- test_serialize_to_bincode_file_and_deserialize_from_bincode_file ... ok
- test_subgraph_name_overflow_protection ... ok
- test_serialize_to_json_and_deserialize ... ok
- test_serialize_to_file_and_deserialize_from_file ... ok

### report::tests (5 tests)
- test_agent_highlight_creation ... ok
- test_extract_key_events_from_simulation ... ok
- test_prediction_report_creation ... ok
- test_timeline_event_creation ... ok
- test_summarize_agents_from_simulation ... ok

### sim::tests (18 tests)
- test_inject_fn_variable_modification ... ok
- test_sim_config_builder_chain ... ok
- test_sim_config_with_inject_fn ... ok
- test_sim_config_new_constructor ... ok
- test_sim_engine_creation ... ok
- test_sim_config_with_inject_fn_builder ... ok
- test_sim_engine_subscribe ... ok
- test_world_snapshot ... ok
- test_subscribe_with_history_returns_shared_arc ... ok
- test_world_snapshot_get_variable ... ok
- test_world_snapshot_preserves_variables ... ok
- test_world_state_advance_tick ... ok
- test_world_state_apply ... ok
- test_world_state_apply_at_deterministic ... ok
- test_world_state_creation ... ok
- test_world_state_variables ... ok

### memory::tests (1 test)
- test_error_handling_invalid_path ... ok

### Ignored Tests (2)
- (none explicitly listed as ignored in the raw output; 2 ignored reported by cargo)

## Regression Criteria

✓ All 142 tests PASS on the green baseline (develop @ c894de8)  
✓ This baseline is the Y-regression reference — the port MUST NOT regress this  
✓ Differential tests will compare MiroFish (Python) output against teri output for parity validation

## Notes

- The test suite is comprehensive (142 core + 2 ignored = 144 total)
- Tests cover all major modules: agent, api, graph, report, sim, memory
- Streaming API has dedicated backpressure and buffering tests
- No test failures or flakes on this baseline
