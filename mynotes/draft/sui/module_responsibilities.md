# Sui 项目模块职责清单（全仓 Rust crates）

> 自动生成：基于各 crate 的 `Cargo.toml`、入口文件 `src/lib.rs`/`src/main.rs` 的 `mod` 声明和 workspace 内部依赖。
>
> 注意：本清单以 **crate = Rust 模块/包** 为粒度（符合 Sui 的工程组织方式）。对核心链路（节点启动/交易执行）我会在 `module_dependencies.md` 里给更细的调用关系图。

## 1. 总览

- 总 crate 数：236
- 角色分布（粗分类）：
  - Move: 75
  - SuiComponent: 75
  - Indexer: 26
  - Core: 20
  - Other: 13
  - Testing: 13
  - Consensus: 5
  - Bridge: 3
  - StorageInfra: 3
  - Crypto: 1
  - Faucet: 1
  - RPC: 1

## 2. 核心模块（建议先读）

| crate | 路径 | 入口 | 顶层子模块（截断） | workspace内依赖（截断） |
|---|---|---|---|---|
| `sui-node` | `crates/sui-node` | `src/lib.rs` | admin, handle, metrics | bin-version, consensus-core, move-vm-config, mysten-common, mysten-metrics, mysten-network, mysten-service, sui-config, sui-core, sui-http, sui-json-rpc, sui-json-rpc-api |
| `sui-core` | `crates/sui-core` | `src/lib.rs` | accumulators, authority, authority_aggregator, authority_client, authority_server, checkpoints, congestion_tracker, consensus_adapter, consensus_handler, consensus_manager, consensus_throughput_calculator, consensus_validator | consensus-config, consensus-core, consensus-types, move-binary-format, move-bytecode-utils, move-core-types, move-symbol-pool, mysten-common, mysten-metrics, mysten-network, shared-crypto, sui-authority-aggregation |
| `sui-types` | `crates/sui-types` | `src/lib.rs` | error, accumulator_event, accumulator_metadata, accumulator_root, address_alias, authenticator_state, balance, balance_change, base_types, bridge, clock, coin | consensus-config, consensus-types, move-binary-format, move-bytecode-utils, move-core-types, move-trace-format, move-vm-profiler, move-vm-test-utils, mysten-common, mysten-metrics, mysten-network, shared-crypto |
| `sui-execution` | `sui-execution` | `src/lib.rs` | executor, verifier, latest, v0, v1, v2, tests | move-abstract-interpreter-v2, move-binary-format, move-bytecode-verifier-meter, move-bytecode-verifier-v0, move-bytecode-verifier-v1, move-bytecode-verifier-v2, move-trace-format, move-vm-config, move-vm-runtime-v0, move-vm-runtime-v1, move-vm-runtime-v2, move-vm-types-v0 |
| `sui-adapter-latest` | `sui-execution/latest/sui-adapter` | `src/lib.rs` | adapter, data_store, error, execution_engine, execution_mode, execution_value, gas_charger, gas_meter, programmable_transactions, static_programmable_transactions, temporary_store, type_layout_resolver | move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-regex-borrow-graph, move-trace-format, move-vm-config, move-vm-profiler, move-vm-runtime, move-vm-types, mysten-common |
| `sui-storage` | `crates/sui-storage` | `src/lib.rs` | blob, http_key_value_store, key_value_store, key_value_store_metrics, mutex_table, object_store, package_object_cache, sharded_lru, write_path_pending_tx_log | move-binary-format, move-bytecode-utils, move-core-types, mysten-metrics, sui-config, sui-json-rpc-types, sui-macros, sui-protocol-config, sui-simulator, sui-test-transaction-builder, sui-types, telemetry-subscribers |
| `typed-store` | `crates/typed-store` | `src/lib.rs` | traits, memstore, metrics, rocks, tidehunter_util, util | mysten-common, mysten-metrics, sui-macros, typed-store-derive, typed-store-error, typed-store-workspace-hack |
| `consensus-core` | `consensus/core` | `src/lib.rs` | ancestor, authority_node, authority_service, base_committer, block, block_manager, block_verifier, commit, commit_consumer, commit_finalizer, commit_observer, commit_syncer | consensus-config, consensus-types, mysten-common, mysten-metrics, mysten-network, shared-crypto, sui-http, sui-macros, sui-protocol-config, sui-tls, telemetry-subscribers, typed-store |
| `sui-network` | `crates/sui-network` | `src/lib.rs` | api, discovery, randomness, state_sync, utils, validator | mysten-common, mysten-metrics, mysten-network, shared-crypto, sui-config, sui-data-ingestion-core, sui-http, sui-macros, sui-protocol-config, sui-simulator, sui-storage, sui-swarm-config |
| `mysten-network` | `crates/mysten-network` | `src/lib.rs` | anemo_connection_monitor, anemo_ext, callback, client, codec, config, grpc_timeout, metrics, multiaddr, quinn_metrics | mysten-metrics, sui-http, sui-tls |
| `sui-json-rpc` | `crates/sui-json-rpc` | `src/lib.rs` | authority_state, balance_changes, bridge_api, coin_api, error, governance_api, indexer_api, logger, metrics, move_utils, object_changes, read_api | move-binary-format, move-bytecode-utils, move-core-types, mysten-metrics, shared-crypto, sui-config, sui-core, sui-display, sui-json, sui-json-rpc-api, sui-json-rpc-types, sui-macros |
| `sui-json-rpc-api` | `crates/sui-json-rpc-api` | `src/lib.rs` | bridge, coin, extended, governance, indexer, move_utils, read, transaction_builder, write | mysten-metrics, sui-json, sui-json-rpc-types, sui-open-rpc, sui-open-rpc-macros, sui-types |
| `sui-json-rpc-types` | `crates/sui-json-rpc-types` | `src/lib.rs` | rpc_types_tests, balance_changes, displays, object_changes, sui_checkpoint, sui_coin, sui_event, sui_extended, sui_governance, sui_move, sui_object, sui_protocol | move-binary-format, move-bytecode-utils, move-command-line-common, move-core-types, move-disassembler, move-ir-types, mysten-metrics, sui-enum-compat-util, sui-json, sui-macros, sui-package-resolver, sui-protocol-config |
| `sui-config` | `crates/sui-config` | `src/lib.rs` | certificate_deny_config, dynamic_transaction_signing_checks, genesis, local_ip_utils, node, node_config_metrics, object_storage_config, p2p, rpc_config, transaction_deny_config, validator_client_monitor_config, verifier_signing_config | consensus-config, move-vm-config, mysten-common, sui-keys, sui-protocol-config, sui-types |
| `sui-protocol-config` | `crates/sui-protocol-config` | `src/lib.rs` |  | move-binary-format, move-core-types, move-vm-config, sui-protocol-config-macros |
| `shared-crypto` | `crates/shared-crypto` | `src/lib.rs` | intent |  |
| `mysten-metrics` | `crates/mysten-metrics` | `src/lib.rs` | guards, histogram, metered_channel, monitored_mpsc, thread_stall_monitor | prometheus-closure-metric |
| `telemetry-subscribers` | `crates/telemetry-subscribers` | `src/lib.rs` | file_exporter, span_latency_prom |  |
| `mysten-service` | `crates/mysten-service` | `src/lib.rs` | health, logging, metrics, server_timing, service | mysten-metrics, telemetry-subscribers |
| `mysten-common` | `crates/mysten-common` | `src/lib.rs` | backoff, decay_moving_average, logging, moving_window, random, random_util, sync | mysten-metrics, sui-macros |

## 3. 全量 crate 索引（按路径/功能粗分组）

### Consensus（4）

| crate | Role(粗) | 路径 | 入口 | 顶层子模块（前10） | workspace内依赖（前10） |
|---|---|---|---|---|---|
| `consensus-config` | Consensus | `consensus/config` | `src/lib.rs` | committee, crypto, parameters, test_committee | mysten-network, shared-crypto |
| `consensus-core` | Core | `consensus/core` | `src/lib.rs` | ancestor, authority_node, authority_service, base_committer, block, block_manager, block_verifier, commit, commit_consumer, commit_finalizer | consensus-config, consensus-types, mysten-common, mysten-metrics, mysten-network, shared-crypto, sui-http, sui-macros, sui-protocol-config, sui-tls |
| `consensus-simtests` | Consensus | `consensus/simtests` | `src/lib.rs` | node | consensus-config, consensus-core, consensus-types, mysten-metrics, mysten-network, sui-config, sui-macros, sui-protocol-config, sui-simulator, telemetry-subscribers |
| `consensus-types` | Consensus | `consensus/types` | `src/lib.rs` | block | consensus-config |

### ExecutionLayer（13）

| crate | Role(粗) | 路径 | 入口 | 顶层子模块（前10） | workspace内依赖（前10） |
|---|---|---|---|---|---|
| `sui-adapter-latest` | Core | `sui-execution/latest/sui-adapter` | `src/lib.rs` | adapter, data_store, error, execution_engine, execution_mode, execution_value, gas_charger, gas_meter, programmable_transactions, static_programmable_transactions | move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-regex-borrow-graph, move-trace-format, move-vm-config, move-vm-profiler, move-vm-runtime |
| `sui-adapter-v0` | SuiComponent | `sui-execution/v0/sui-adapter` | `src/lib.rs` | adapter, error, execution_engine, execution_mode, execution_value, gas_charger, gas_meter, programmable_transactions, temporary_store, type_layout_resolver | move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config, move-vm-profiler, move-vm-runtime, move-vm-types, sui-macros |
| `sui-adapter-v1` | SuiComponent | `sui-execution/v1/sui-adapter` | `src/lib.rs` | adapter, error, execution_engine, execution_mode, execution_value, gas_charger, gas_meter, programmable_transactions, temporary_store, type_layout_resolver | move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config, move-vm-profiler, move-vm-runtime, move-vm-types, sui-macros |
| `sui-adapter-v2` | SuiComponent | `sui-execution/v2/sui-adapter` | `src/lib.rs` | adapter, error, execution_engine, execution_mode, execution_value, gas_charger, gas_meter, programmable_transactions, temporary_store, type_layout_resolver | move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config, move-vm-profiler, move-vm-runtime, move-vm-types, sui-macros |
| `sui-execution-cut` | SuiComponent | `sui-execution/cut` | `src/main.rs` | args, path, plan |  |
| `sui-move-natives-latest` | Testing | `sui-execution/latest/sui-move-natives` | `src/lib.rs` | accumulator, address, config, crypto, dynamic_field, event, funds_accumulator, object, object_runtime, protocol_config | move-binary-format, move-core-types, move-stdlib-natives, move-vm-runtime, move-vm-types, sui-protocol-config, sui-types |
| `sui-move-natives-v0` | SuiComponent | `sui-execution/v0/sui-move-natives` | `src/lib.rs` | address, crypto, dynamic_field, event, object, object_runtime, test_scenario, test_utils, transfer, tx_context | move-binary-format, move-core-types, move-stdlib-natives, move-vm-runtime, move-vm-types, sui-protocol-config, sui-types |
| `sui-move-natives-v1` | SuiComponent | `sui-execution/v1/sui-move-natives` | `src/lib.rs` | address, crypto, dynamic_field, event, object, object_runtime, test_scenario, test_utils, transfer, tx_context | move-binary-format, move-core-types, move-stdlib-natives, move-vm-runtime, move-vm-types, sui-protocol-config, sui-types |
| `sui-move-natives-v2` | SuiComponent | `sui-execution/v2/sui-move-natives` | `src/lib.rs` | address, crypto, dynamic_field, event, object, object_runtime, test_scenario, test_utils, transfer, tx_context | move-binary-format, move-core-types, move-stdlib-natives, move-vm-runtime, move-vm-types, sui-protocol-config, sui-types |
| `sui-verifier-latest` | Testing | `sui-execution/latest/sui-verifier` | `src/lib.rs` | verifier, entry_points_verifier, global_storage_access_verifier, id_leak_verifier, meter, one_time_witness_verifier, private_generics, private_generics_verifier_v2, struct_with_key_verifier | move-abstract-stack, move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config, sui-protocol-config, sui-types |
| `sui-verifier-v0` | SuiComponent | `sui-execution/v0/sui-verifier` | `src/lib.rs` | verifier, entry_points_verifier, global_storage_access_verifier, id_leak_verifier, meter, one_time_witness_verifier, private_generics, struct_with_key_verifier | move-abstract-stack, move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config, sui-protocol-config, sui-types |
| `sui-verifier-v1` | SuiComponent | `sui-execution/v1/sui-verifier` | `src/lib.rs` | verifier, entry_points_verifier, global_storage_access_verifier, id_leak_verifier, meter, one_time_witness_verifier, private_generics, struct_with_key_verifier | move-abstract-stack, move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config, sui-types |
| `sui-verifier-v2` | SuiComponent | `sui-execution/v2/sui-verifier` | `src/lib.rs` | verifier, entry_points_verifier, global_storage_access_verifier, id_leak_verifier, meter, one_time_witness_verifier, private_generics, struct_with_key_verifier | move-abstract-stack, move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config, sui-protocol-config, sui-types |

### Move（76）

| crate | Role(粗) | 路径 | 入口 | 顶层子模块（前10） | workspace内依赖（前10） |
|---|---|---|---|---|---|
| `bytecode-interpreter-crypto` | Crypto | `external-crates/move/crates/bytecode-interpreter-crypto` | `src/lib.rs` |  |  |
| `bytecode-verifier-libfuzzer` | Move | `external-crates/move/crates/bytecode-verifier-libfuzzer` | `` |  | move-binary-format, move-bytecode-verifier, move-core-types |
| `bytecode-verifier-prop-tests` | Move | `external-crates/move/crates/bytecode-verifier-prop-tests` | `src/lib.rs` | unit_tests | invalid-mutations, move-binary-format, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config |
| `bytecode-verifier-tests` | Move | `external-crates/move/crates/bytecode-verifier-tests` | `src/lib.rs` | support, unit_tests | move-abstract-interpreter, move-binary-format, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config |
| `bytecode-verifier-tests-v0` | Move | `external-crates/move/move-execution/v0/crates/bytecode-verifier-tests` | `src/lib.rs` | support, unit_tests | move-abstract-interpreter, move-binary-format, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config |
| `bytecode-verifier-tests-v1` | Move | `external-crates/move/move-execution/v1/crates/bytecode-verifier-tests` | `src/lib.rs` | support, unit_tests | move-abstract-interpreter, move-binary-format, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config |
| `bytecode-verifier-tests-v2` | Move | `external-crates/move/move-execution/v2/crates/bytecode-verifier-tests` | `src/lib.rs` | support, unit_tests | move-abstract-interpreter, move-binary-format, move-bytecode-verifier, move-bytecode-verifier-meter, move-core-types, move-vm-config |
| `bytecode-verifier-transactional-tests` | Move | `external-crates/move/crates/bytecode-verifier-transactional-tests` | `src/lib.rs` |  | move-transactional-test-runner |
| `enum-compat-util` | Move | `external-crates/move/crates/enum-compat-util` | `src/lib.rs` |  |  |
| `invalid-mutations` | Move | `external-crates/move/crates/invalid-mutations` | `src/lib.rs` | bounds, helpers, signature | move-binary-format, move-core-types |
| `jsonrpc` | Move | `external-crates/move/crates/jsonrpc` | `src/lib.rs` | client, types |  |
| `language-benchmarks` | Move | `external-crates/move/crates/language-benchmarks` | `src/lib.rs` | measurement, move_vm | move-binary-format, move-compiler, move-core-types, move-stdlib, move-stdlib-natives, move-vm-runtime, move-vm-test-utils, move-vm-types |
| `module-generation` | Move | `external-crates/move/crates/module-generation` | `src/lib.rs` | generator, options, padding, utils | move-binary-format, move-bytecode-verifier, move-core-types, move-ir-to-bytecode, move-ir-types, move-symbol-pool |
| `move-abstract-interpreter` | Move | `external-crates/move/crates/move-abstract-interpreter` | `src/lib.rs` | absint, control_flow_graph |  |
| `move-abstract-interpreter-v2` | Move | `external-crates/move/move-execution/v2/crates/move-abstract-interpreter` | `src/lib.rs` | control_flow_graph, unit_tests | move-binary-format |
| `move-abstract-stack` | Move | `external-crates/move/crates/move-abstract-stack` | `src/lib.rs` | unit_tests |  |
| `move-analyzer` | Move | `external-crates/move/crates/move-analyzer` | `src/lib.rs` | analysis, analyzer, code_action, compiler_info, completions, context, diagnostics, inlay_hints, symbols, utils | move-command-line-common, move-compiler, move-core-types, move-ir-types, move-package-alt, move-package-alt-compilation, move-symbol-pool |
| `move-binary-format` | Move | `external-crates/move/crates/move-binary-format` | `src/lib.rs` | binary_config, check_bounds, compatibility, compatibility_mode, errors, constant, deserializer, file_format, file_format_common, internals | enum-compat-util, move-abstract-interpreter, move-core-types, move-proc-macros |
| `move-borrow-graph` | Move | `external-crates/move/crates/move-borrow-graph` | `src/lib.rs` | graph, paths, references, shared |  |
| `move-bytecode-source-map` | Move | `external-crates/move/crates/move-bytecode-source-map` | `src/lib.rs` | mapping, marking, source_map, utils | move-binary-format, move-command-line-common, move-core-types, move-ir-types, move-symbol-pool |
| `move-bytecode-utils` | Move | `external-crates/move/crates/move-bytecode-utils` | `src/lib.rs` | layout, module_cache | move-binary-format, move-core-types |
| `move-bytecode-verifier` | Move | `external-crates/move/crates/move-bytecode-verifier` | `src/lib.rs` | ability_cache, ability_field_requirements, absint, check_duplication, code_unit_verifier, constants, control_flow, control_flow_v5, cyclic_dependencies, data_defs | move-abstract-interpreter, move-abstract-stack, move-binary-format, move-borrow-graph, move-bytecode-verifier-meter, move-core-types, move-regex-borrow-graph, move-vm-config |
| `move-bytecode-verifier-meter` | Move | `external-crates/move/crates/move-bytecode-verifier-meter` | `src/lib.rs` | bound, dummy | move-binary-format, move-core-types, move-vm-config |
| `move-bytecode-verifier-v0` | Move | `external-crates/move/move-execution/v0/crates/move-bytecode-verifier` | `src/lib.rs` | ability_field_requirements, absint, check_duplication, code_unit_verifier, constants, control_flow, control_flow_v5, cyclic_dependencies, dependencies, friends | move-abstract-interpreter, move-abstract-stack, move-binary-format, move-borrow-graph, move-bytecode-verifier-meter, move-core-types, move-vm-config |
| `move-bytecode-verifier-v1` | Move | `external-crates/move/move-execution/v1/crates/move-bytecode-verifier` | `src/lib.rs` | ability_field_requirements, absint, check_duplication, code_unit_verifier, constants, control_flow, control_flow_v5, cyclic_dependencies, dependencies, friends | move-abstract-interpreter, move-abstract-stack, move-binary-format, move-borrow-graph, move-bytecode-verifier-meter, move-core-types, move-vm-config |
| `move-bytecode-verifier-v2` | Move | `external-crates/move/move-execution/v2/crates/move-bytecode-verifier` | `src/lib.rs` | ability_field_requirements, absint, check_duplication, code_unit_verifier, constants, control_flow, control_flow_v5, cyclic_dependencies, dependencies, friends | move-abstract-interpreter, move-abstract-stack, move-binary-format, move-borrow-graph, move-bytecode-verifier-meter, move-core-types, move-vm-config |
| `move-bytecode-viewer` | Move | `external-crates/move/crates/move-bytecode-viewer` | `src/lib.rs` | bytecode_viewer, interfaces, source_viewer, tui, viewer | move-binary-format, move-bytecode-source-map, move-disassembler |
| `move-cli` | Move | `external-crates/move/crates/move-cli` | `src/lib.rs` | base, sandbox | move-binary-format, move-bytecode-source-map, move-bytecode-utils, move-bytecode-verifier, move-bytecode-viewer, move-command-line-common, move-compiler, move-core-types, move-coverage, move-decompiler |
| `move-command-line-common` | Move | `external-crates/move/crates/move-command-line-common` | `src/lib.rs` | character_sets, display, env, error_bitset, files, interactive, testing | move-binary-format, move-core-types |
| `move-compiler` | Move | `external-crates/move/crates/move-compiler` | `src/lib.rs` | cfgir, command_line, compiled_unit, diagnostics, editions, expansion, hlir, interface_generator, ir_translation, linters | move-abstract-interpreter, move-binary-format, move-borrow-graph, move-bytecode-source-map, move-bytecode-verifier, move-command-line-common, move-core-types, move-ir-to-bytecode, move-ir-types, move-proc-macros |
| `move-compiler-transactional-tests` | Move | `external-crates/move/crates/move-compiler-transactional-tests` | `src/lib.rs` |  | move-transactional-test-runner |
| `move-core-types` | Move | `external-crates/move/crates/move-core-types` | `src/lib.rs` | account_address, annotated_extractor, annotated_value, annotated_visitor, effects, gas_algebra, identifier, language_storage, metadata, parsing | enum-compat-util, move-proc-macros |
| `move-coverage` | Move | `external-crates/move/crates/move-coverage` | `src/lib.rs` | coverage_map, differential_coverage, lcov, source_coverage, summary | move-abstract-interpreter, move-binary-format, move-bytecode-source-map, move-bytecode-verifier, move-command-line-common, move-compiler, move-core-types, move-ir-types, move-trace-format |
| `move-decompiler` | Move | `external-crates/move/crates/move-decompiler` | `src/lib.rs` | ast, refinement, structuring, config, pretty_printer, testing, translate | move-abstract-interpreter, move-binary-format, move-command-line-common, move-core-types, move-disassembler, move-ir-types, move-model, move-model-2, move-package-alt, move-package-alt-compilation |
| `move-disassembler` | Move | `external-crates/move/crates/move-disassembler` | `src/lib.rs` | disassembler | move-abstract-interpreter, move-binary-format, move-bytecode-source-map, move-command-line-common, move-compiler, move-core-types, move-coverage, move-ir-types, move-symbol-pool |
| `move-docgen` | Move | `external-crates/move/crates/move-docgen` | `src/lib.rs` | code_writer, docgen | move-binary-format, move-compiler, move-core-types, move-ir-types, move-model-2, move-symbol-pool |
| `move-docgen-tests` | Move | `external-crates/move/crates/move-docgen-tests` | `src/lib.rs` |  | move-binary-format, move-command-line-common, move-compiler, move-core-types, move-docgen, move-ir-types, move-model-2, move-package-alt, move-package-alt-compilation, move-symbol-pool |
| `move-ir-compiler` | Move | `external-crates/move/crates/move-ir-compiler` | `src/lib.rs` | util, unit_tests | move-abstract-interpreter, move-binary-format, move-bytecode-source-map, move-bytecode-verifier, move-command-line-common, move-core-types, move-ir-to-bytecode |
| `move-ir-compiler-transactional-tests` | Move | `external-crates/move/crates/move-ir-compiler-transactional-tests` | `` |  | move-transactional-test-runner |
| `move-ir-to-bytecode` | Move | `external-crates/move/crates/move-ir-to-bytecode` | `src/lib.rs` | compiler, context, parser | move-binary-format, move-bytecode-source-map, move-command-line-common, move-core-types, move-ir-to-bytecode-syntax, move-ir-types, move-symbol-pool |
| `move-ir-to-bytecode-syntax` | Move | `external-crates/move/crates/move-ir-to-bytecode-syntax` | `src/lib.rs` | lexer, syntax | move-command-line-common, move-core-types, move-ir-types, move-symbol-pool |
| `move-ir-types` | Move | `external-crates/move/crates/move-ir-types` | `src/lib.rs` | ast, location | move-command-line-common, move-core-types, move-symbol-pool |
| `move-model` | Move | `external-crates/move/crates/move-model` | `src/lib.rs` | ast, builder, code_writer, exp_generator, model, options, pragmas, symbol, ty, well_known | move-binary-format, move-bytecode-source-map, move-command-line-common, move-compiler, move-core-types, move-disassembler, move-ir-types, move-symbol-pool |
| `move-model-2` | Move | `external-crates/move/crates/move-model-2` | `src/lib.rs` | compiled_model, display, model, normalized, pretty_printer, source_kind, source_model, summary | move-binary-format, move-bytecode-source-map, move-command-line-common, move-compiler, move-core-types, move-disassembler, move-ir-types, move-symbol-pool |
| `move-package` | Move | `external-crates/move/crates/move-package` | `src/lib.rs` | package_lock, compilation, lock_file, migration, package_hooks, resolution, source_package | move-binary-format, move-bytecode-source-map, move-bytecode-utils, move-command-line-common, move-compiler, move-core-types, move-disassembler, move-docgen, move-model-2, move-symbol-pool |
| `move-package-alt` | Move | `external-crates/move/crates/move-package-alt` | `src/lib.rs` | cli, compatibility, dependency, errors, flavor, git, graph, package, schema, test_utils | jsonrpc, move-command-line-common, move-compiler, move-core-types, move-symbol-pool |
| `move-package-alt-compilation` | Move | `external-crates/move/crates/move-package-alt-compilation` | `src/lib.rs` | build_config, build_plan, compilation, compiled_package, documentation, layout, lint_flag, migrate, model_builder, on_disk_package | move-binary-format, move-bytecode-source-map, move-bytecode-utils, move-command-line-common, move-compiler, move-core-types, move-disassembler, move-docgen, move-model-2, move-package-alt |
| `move-proc-macros` | Move | `external-crates/move/crates/move-proc-macros` | `src/lib.rs` |  | enum-compat-util |
| `move-regex-borrow-graph` | Move | `external-crates/move/crates/move-regex-borrow-graph` | `src/lib.rs` | collections, references, regex, tests | move-binary-format, move-command-line-common, move-core-types |
| `move-stackless-bytecode` | Move | `external-crates/move/crates/move-stackless-bytecode` | `src/lib.rs` | access_path, access_path_trie, annotations, borrow_analysis, clean_and_optimize, compositional_analysis, dataflow_analysis, dataflow_domains, debug_instrumentation, eliminate_imm_refs | move-binary-format, move-command-line-common, move-compiler, move-core-types, move-model, move-stdlib |
| `move-stackless-bytecode-2` | Move | `external-crates/move/crates/move-stackless-bytecode-2` | `src/lib.rs` | ast, translate | move-abstract-interpreter, move-binary-format, move-command-line-common, move-core-types, move-disassembler, move-ir-types, move-model, move-model-2, move-package-alt, move-package-alt-compilation |
| `move-stdlib` | Move | `external-crates/move/crates/move-stdlib` | `src/lib.rs` | tests, utils | move-binary-format, move-cli, move-command-line-common, move-core-types, move-docgen, move-package-alt, move-package-alt-compilation, move-stdlib-natives, move-unit-test, move-vm-runtime |
| `move-stdlib-natives` | Move | `external-crates/move/crates/move-stdlib-natives` | `src/lib.rs` | bcs, debug, hash, signer, string, type_name, unit_test, vector, helpers | move-binary-format, move-core-types, move-vm-runtime, move-vm-types |
| `move-stdlib-natives-v0` | Move | `external-crates/move/move-execution/v0/crates/move-stdlib-natives` | `src/lib.rs` | bcs, debug, hash, signer, string, type_name, unit_test, vector, helpers | move-binary-format, move-core-types, move-vm-runtime, move-vm-types |
| `move-stdlib-natives-v1` | Move | `external-crates/move/move-execution/v1/crates/move-stdlib-natives` | `src/lib.rs` | bcs, debug, hash, signer, string, type_name, unit_test, vector, helpers | move-binary-format, move-core-types, move-vm-runtime, move-vm-types |
| `move-stdlib-natives-v2` | Move | `external-crates/move/move-execution/v2/crates/move-stdlib-natives` | `src/lib.rs` | bcs, debug, hash, signer, string, type_name, unit_test, vector, helpers | move-binary-format, move-core-types, move-vm-runtime, move-vm-types |
| `move-symbol-pool` | Move | `external-crates/move/crates/move-symbol-pool` | `src/lib.rs` | pool, symbol |  |
| `move-trace-format` | Move | `external-crates/move/crates/move-trace-format` | `src/lib.rs` | format, interface, memory_tracer, value | move-binary-format, move-core-types |
| `move-transactional-test-runner` | Move | `external-crates/move/crates/move-transactional-test-runner` | `src/lib.rs` | framework, tasks, vm_test_harness | move-binary-format, move-bytecode-source-map, move-cli, move-command-line-common, move-compiler, move-core-types, move-disassembler, move-ir-compiler, move-ir-types, move-stdlib |
| `move-unit-test` | Move | `external-crates/move/crates/move-unit-test` | `src/lib.rs` | cargo_runner, extensions, test_reporter, test_runner | move-binary-format, move-bytecode-utils, move-command-line-common, move-compiler, move-core-types, move-ir-types, move-model, move-stdlib, move-stdlib-natives, move-symbol-pool |
| `move-vm-config` | Move | `external-crates/move/crates/move-vm-config` | `src/lib.rs` | runtime, verifier | move-binary-format |
| `move-vm-integration-tests` | Move | `external-crates/move/crates/move-vm-integration-tests` | `src/lib.rs` | compiler, tests | move-binary-format, move-bytecode-verifier, move-compiler, move-core-types, move-ir-to-bytecode, move-stdlib, move-stdlib-natives, move-vm-config, move-vm-profiler, move-vm-runtime |
| `move-vm-profiler` | Move | `external-crates/move/crates/move-vm-profiler` | `src/lib.rs` | trace_converter | move-trace-format, move-vm-config |
| `move-vm-runtime` | Move | `external-crates/move/crates/move-vm-runtime` | `src/lib.rs` | data_cache, interpreter, loader, logging, move_vm, native_extensions, native_functions, runtime, session, tracing | move-binary-format, move-bytecode-verifier, move-compiler, move-core-types, move-ir-compiler, move-trace-format, move-vm-config, move-vm-types |
| `move-vm-runtime-v0` | Move | `external-crates/move/move-execution/v0/crates/move-vm-runtime` | `src/lib.rs` | data_cache, interpreter, loader, logging, move_vm, native_extensions, native_functions, runtime, session, tracing | move-binary-format, move-bytecode-verifier, move-compiler, move-core-types, move-ir-compiler, move-vm-config, move-vm-types |
| `move-vm-runtime-v1` | Move | `external-crates/move/move-execution/v1/crates/move-vm-runtime` | `src/lib.rs` | data_cache, interpreter, loader, logging, move_vm, native_extensions, native_functions, runtime, session, tracing | move-binary-format, move-bytecode-verifier, move-compiler, move-core-types, move-ir-compiler, move-vm-config, move-vm-types |
| `move-vm-runtime-v2` | Move | `external-crates/move/move-execution/v2/crates/move-vm-runtime` | `src/lib.rs` | data_cache, interpreter, loader, logging, move_vm, native_extensions, native_functions, runtime, session, tracing | move-binary-format, move-bytecode-verifier, move-compiler, move-core-types, move-ir-compiler, move-vm-config, move-vm-types |
| `move-vm-test-utils` | Move | `external-crates/move/crates/move-vm-test-utils` | `src/lib.rs` | storage, gas_schedule, tiered_gas_schedule | move-binary-format, move-core-types, move-vm-profiler, move-vm-types |
| `move-vm-transactional-tests` | Move | `external-crates/move/crates/move-vm-transactional-tests` | `src/lib.rs` |  | move-transactional-test-runner |
| `move-vm-types` | Move | `external-crates/move/crates/move-vm-types` | `src/lib.rs` | data_store, gas, loaded_data, natives, values, views, unit_tests | move-binary-format, move-core-types, move-vm-profiler |
| `move-vm-types-v0` | Move | `external-crates/move/move-execution/v0/crates/move-vm-types` | `src/lib.rs` | data_store, gas, loaded_data, natives, values, views, unit_tests | move-binary-format, move-core-types |
| `move-vm-types-v1` | Move | `external-crates/move/move-execution/v1/crates/move-vm-types` | `src/lib.rs` | data_store, gas, loaded_data, natives, values, views, unit_tests | move-binary-format, move-core-types |
| `move-vm-types-v2` | Move | `external-crates/move/move-execution/v2/crates/move-vm-types` | `src/lib.rs` | data_store, gas, loaded_data, natives, values, views, unit_tests | move-binary-format, move-core-types |
| `serializer-tests` | Move | `external-crates/move/crates/serializer-tests` | `src/lib.rs` |  | move-binary-format |
| `test-generation` | Move | `external-crates/move/crates/test-generation` | `src/lib.rs` | abstract_state, borrow_graph, bytecode_generator, config, control_flow_graph, error, summaries, transitions | module-generation, move-binary-format, move-bytecode-verifier, move-compiler, move-core-types, move-stdlib, move-stdlib-natives, move-vm-runtime, move-vm-test-utils, move-vm-types |
| `tree-sitter-move` | Move | `external-crates/move/tooling/tree-sitter` | `` |  |  |

### MystenInfra（4）

| crate | Role(粗) | 路径 | 入口 | 顶层子模块（前10） | workspace内依赖（前10） |
|---|---|---|---|---|---|
| `mysten-common` | Core | `crates/mysten-common` | `src/lib.rs` | backoff, decay_moving_average, logging, moving_window, random, random_util, sync | mysten-metrics, sui-macros |
| `mysten-metrics` | Core | `crates/mysten-metrics` | `src/lib.rs` | guards, histogram, metered_channel, monitored_mpsc, thread_stall_monitor | prometheus-closure-metric |
| `mysten-network` | Core | `crates/mysten-network` | `src/lib.rs` | anemo_connection_monitor, anemo_ext, callback, client, codec, config, grpc_timeout, metrics, multiaddr, quinn_metrics | mysten-metrics, sui-http, sui-tls |
| `mysten-service` | Core | `crates/mysten-service` | `src/lib.rs` | health, logging, metrics, server_timing, service | mysten-metrics, telemetry-subscribers |

### Other（23）

| crate | Role(粗) | 路径 | 入口 | 顶层子模块（前10） | workspace内依赖（前10） |
|---|---|---|---|---|---|
| `anemo-benchmark` | Other | `crates/anemo-benchmark` | `src/lib.rs` | server | mysten-network, telemetry-subscribers |
| `basic-sui-indexer` | Indexer | `examples/rust/basic-sui-indexer` | `src/main.rs` | models, handlers, schema | sui-indexer-alt-framework |
| `bin-version` | Other | `crates/bin-version` | `src/lib.rs` |  |  |
| `clickhouse-sui-indexer` | Indexer | `examples/rust/clickhouse-sui-indexer` | `src/main.rs` | handlers, store | sui-indexer-alt-framework, sui-indexer-alt-framework-store-traits, sui-types |
| `consensus-framework` | Consensus | `notes/experiments/consensus-framework` | `src/lib.rs` | traits, types, error, mysticeti_adapter | consensus-config, consensus-core, consensus-types, sui-types |
| `consensus-study` | Consensus | `notes/experiments/consensus-poc/consensus-study` | `src/lib.rs` | dag_builder | consensus-config, consensus-core, consensus-types |
| `custom-indexer` | Indexer | `examples/custom-indexer/rust` | `` |  |  |
| `dag-visualizer` | Other | `notes/experiments/dag-visualizer` | `src/main.rs` |  | consensus-study |
| `dex-rollup` | Other | `notes/experiments/dex-rollup` | `src/lib.rs` | balance, engine, error, fraud_proof, orderbook, sequencer, types | consensus-framework, sui-types |
| `prometheus-closure-metric` | Other | `crates/prometheus-closure-metric` | `src/lib.rs` |  |  |
| `rust-client` | Other | `examples/regulated-coin/rust-client` | `src/lib.rs` | gas, tx_run | move-core-types, shared-crypto, sui-config, sui-keys, sui-sdk |
| `shared-crypto` | Core | `crates/shared-crypto` | `src/lib.rs` | intent |  |
| `simple-token-chain` | Other | `notes/experiments/simple-token-chain` | `src/lib.rs` | error, executor, node, rpc, types | consensus-framework |
| `simulacrum` | Other | `crates/simulacrum` | `src/lib.rs` | epoch_state, store | move-binary-format, move-bytecode-utils, move-core-types, shared-crypto, sui-config, sui-core, sui-execution, sui-framework, sui-framework-snapshot, sui-genesis-builder |
| `sui` | Other | `crates/sui` | `src/lib.rs` | client_commands, client_ptb, clever_error_rendering, displays, fire_drill, genesis_ceremony, genesis_inspector, keytool, mvr_resolver, sui_commands | bin-version, move-analyzer, move-binary-format, move-bytecode-source-map, move-bytecode-verifier-meter, move-cli, move-command-line-common, move-compiler, move-core-types, move-ir-types |
| `sui-execution` | Core | `sui-execution` | `src/lib.rs` | executor, verifier, latest, v0, v1, v2, tests | move-abstract-interpreter-v2, move-binary-format, move-bytecode-verifier-meter, move-bytecode-verifier-v0, move-bytecode-verifier-v1, move-bytecode-verifier-v2, move-trace-format, move-vm-config, move-vm-runtime-v0, move-vm-runtime-v1 |
| `suins-indexer` | Indexer | `crates/suins-indexer` | `src/lib.rs` | indexer, models, schema | move-core-types, mysten-metrics, mysten-service, sui-data-ingestion-core, sui-name-service, sui-storage, sui-types, telemetry-subscribers |
| `telemetry-subscribers` | Core | `crates/telemetry-subscribers` | `src/lib.rs` | file_exporter, span_latency_prom |  |
| `test-cluster` | Other | `crates/test-cluster` | `src/lib.rs` | test_indexer_handle | move-binary-format, mysten-common, sui-config, sui-core, sui-framework, sui-indexer, sui-json-rpc, sui-json-rpc-api, sui-json-rpc-types, sui-keys |
| `tic-tac-toe` | Other | `examples/tic-tac-toe/cli` | `src/lib.rs` | command | move-core-types, shared-crypto, sui-keys, sui-sdk, sui-types |
| `transaction-fuzzer` | Other | `crates/transaction-fuzzer` | `src/lib.rs` | account_universe, config_fuzzer, executor, programmable_transaction_gen, transaction_data_gen, type_arg_fuzzer | move-core-types, sui-core, sui-move-build, sui-protocol-config, sui-types |
| `walrus-attributes-indexer` | Indexer | `examples/rust/walrus-attributes-indexer` | `src/lib.rs` | schema, storage, types, handlers | move-core-types, sui-indexer-alt-framework |
| `x` | Other | `crates/x` | `src/main.rs` | external_crates_tests, lint |  |

### StorageInfra（4）

| crate | Role(粗) | 路径 | 入口 | 顶层子模块（前10） | workspace内依赖（前10） |
|---|---|---|---|---|---|
| `typed-store` | Core | `crates/typed-store` | `src/lib.rs` | traits, memstore, metrics, rocks, tidehunter_util, util | mysten-common, mysten-metrics, sui-macros, typed-store-derive, typed-store-error, typed-store-workspace-hack |
| `typed-store-derive` | StorageInfra | `crates/typed-store-derive` | `src/lib.rs` |  |  |
| `typed-store-error` | StorageInfra | `crates/typed-store-error` | `src/lib.rs` | errors |  |
| `typed-store-workspace-hack` | StorageInfra | `crates/typed-store-workspace-hack` | `src/lib.rs` |  |  |

### SuiCrates（112）

| crate | Role(粗) | 路径 | 入口 | 顶层子模块（前10） | workspace内依赖（前10） |
|---|---|---|---|---|---|
| `sui-adapter-transactional-tests` | Testing | `crates/sui-adapter-transactional-tests` | `src/lib.rs` |  | sui-transactional-test-runner |
| `sui-analytics-indexer` | Indexer | `crates/sui-analytics-indexer` | `src/lib.rs` | config, handlers, indexer, metrics, package_store, pipeline, progress_monitoring, schema, store, tables | move-binary-format, move-bytecode-utils, move-core-types, mysten-metrics, sui-analytics-indexer-derive, sui-futures, sui-indexer, sui-indexer-alt-framework, sui-indexer-alt-framework-store-traits, sui-indexer-alt-metrics |
| `sui-analytics-indexer-derive` | Indexer | `crates/sui-analytics-indexer-derive` | `src/lib.rs` |  |  |
| `sui-authority-aggregation` | SuiComponent | `crates/sui-authority-aggregation` | `src/lib.rs` |  | mysten-metrics, sui-types |
| `sui-aws-orchestrator` | SuiComponent | `crates/sui-aws-orchestrator` | `src/main.rs` | benchmark, client, display, error, faults, logs, measurement, monitor, orchestrator, protocol | mysten-metrics, sui-config, sui-swarm-config, sui-types |
| `sui-benchmark` | SuiComponent | `crates/sui-benchmark` | `src/lib.rs` | bank, benchmark_setup, drivers, fullnode_reconfig_observer, in_memory_wallet, options, system_state_observer, util, workloads | move-core-types, mysten-metrics, sui-config, sui-core, sui-json-rpc-types, sui-keys, sui-move-build, sui-network, sui-protocol-config, sui-sdk |
| `sui-bridge` | Bridge | `crates/sui-bridge` | `src/lib.rs` | abi, action_executor, client, config, crypto, encoding, error, eth_client, eth_syncer, eth_transaction_builder | bin-version, move-core-types, mysten-common, mysten-metrics, shared-crypto, sui-authority-aggregation, sui-config, sui-json-rpc-api, sui-json-rpc-types, sui-keys |
| `sui-bridge-cli` | Bridge | `crates/sui-bridge-cli` | `src/lib.rs` |  | move-core-types, shared-crypto, sui-bridge, sui-config, sui-json-rpc-types, sui-keys, sui-sdk, sui-types, telemetry-subscribers |
| `sui-bridge-indexer` | Indexer | `crates/sui-bridge-indexer` | `src/lib.rs` | config, metrics, postgres_manager, storage, sui_transaction_handler, sui_transaction_queries, types, eth_bridge_indexer, sui_bridge_indexer | mysten-metrics, sui-bridge, sui-bridge-schema, sui-config, sui-data-ingestion-core, sui-indexer, sui-indexer-builder, sui-json-rpc-types, sui-pg-db, sui-sdk |
| `sui-bridge-indexer-alt` | Indexer | `crates/sui-bridge-indexer-alt` | `src/lib.rs` | handlers, metrics | move-core-types, mysten-metrics, sui-bridge, sui-bridge-schema, sui-indexer-alt-framework, sui-indexer-alt-metrics, telemetry-subscribers |
| `sui-bridge-schema` | Bridge | `crates/sui-bridge-schema` | `src/lib.rs` | models, schema | sui-field-count, sui-indexer-builder |
| `sui-checkpoint-blob-indexer` | Indexer | `crates/sui-checkpoint-blob-indexer` | `src/lib.rs` | handlers | sui-config, sui-indexer-alt-framework, sui-indexer-alt-framework-store-traits, sui-indexer-alt-metrics, sui-indexer-alt-object-store, sui-types |
| `sui-cluster-test` | Testing | `crates/sui-cluster-test` | `src/lib.rs` | cluster, config, faucet, helper, test_case, wallet_client | move-core-types, shared-crypto, sui-config, sui-core, sui-faucet, sui-graphql-rpc, sui-indexer, sui-json, sui-json-rpc-types, sui-keys |
| `sui-config` | Core | `crates/sui-config` | `src/lib.rs` | certificate_deny_config, dynamic_transaction_signing_checks, genesis, local_ip_utils, node, node_config_metrics, object_storage_config, p2p, rpc_config, transaction_deny_config | consensus-config, move-vm-config, mysten-common, sui-keys, sui-protocol-config, sui-types |
| `sui-core` | Core | `crates/sui-core` | `src/lib.rs` | accumulators, authority, authority_aggregator, authority_client, authority_server, checkpoints, congestion_tracker, consensus_adapter, consensus_handler, consensus_manager | consensus-config, consensus-core, consensus-types, move-binary-format, move-bytecode-utils, move-core-types, move-symbol-pool, mysten-common, mysten-metrics, mysten-network |
| `sui-cost` | SuiComponent | `crates/sui-cost` | `` |  | move-disassembler, sui-config, sui-json-rpc-types, sui-move-build, sui-swarm-config, sui-test-transaction-builder, sui-types, test-cluster |
| `sui-data-ingestion` | SuiComponent | `crates/sui-data-ingestion` | `src/lib.rs` | progress_store, workers | mysten-metrics, sui-data-ingestion-core, sui-kvstore, sui-storage, sui-types, telemetry-subscribers |
| `sui-data-ingestion-core` | SuiComponent | `crates/sui-data-ingestion-core` | `src/lib.rs` | executor, metrics, progress_store, reader, reducer, tests, util, worker_pool | mysten-metrics, sui-protocol-config, sui-rpc-api, sui-storage, sui-types |
| `sui-data-store` | SuiComponent | `crates/sui-data-store` | `src/lib.rs` | gql_queries, node, stores | sui-config, sui-types |
| `sui-deepbook-indexer` | Indexer | `crates/sui-deepbook-indexer` | `src/lib.rs` | config, error, events, metrics, models, postgres_manager, schema, server, sui_deepbook_indexer, types | mysten-metrics, sui-config, sui-data-ingestion-core, sui-indexer-builder, sui-json-rpc-types, sui-sdk, sui-types, telemetry-subscribers |
| `sui-default-config` | SuiComponent | `crates/sui-default-config` | `src/lib.rs` |  |  |
| `sui-display` | SuiComponent | `crates/sui-display` | `src/lib.rs` | v1, v2 | move-core-types, sui-json-rpc-types, sui-types |
| `sui-e2e-tests` | Testing | `crates/sui-e2e-tests` | `` |  | move-binary-format, move-core-types, mysten-common, mysten-metrics, shared-crypto, sui-config, sui-core, sui-framework, sui-framework-snapshot, sui-json-rpc |
| `sui-enum-compat-util` | SuiComponent | `crates/sui-enum-compat-util` | `src/lib.rs` |  |  |
| `sui-faucet` | Faucet | `crates/sui-faucet` | `src/lib.rs` | app_state, errors, faucet_config, local_faucet, server, types | bin-version, shared-crypto, sui-config, sui-keys, sui-sdk, test-cluster |
| `sui-field-count` | SuiComponent | `crates/sui-field-count` | `src/lib.rs` |  | sui-field-count-derive |
| `sui-field-count-derive` | SuiComponent | `crates/sui-field-count-derive` | `src/lib.rs` |  |  |
| `sui-framework` | SuiComponent | `crates/sui-framework` | `src/lib.rs` |  | move-binary-format, move-compiler, move-core-types, move-package-alt-compilation, sui-config, sui-move-build, sui-package-alt, sui-types |
| `sui-framework-snapshot` | SuiComponent | `crates/sui-framework-snapshot` | `src/lib.rs` |  | bin-version, sui-framework, sui-move-build, sui-protocol-config, sui-types |
| `sui-framework-tests` | Testing | `crates/sui-framework-tests` | `src/lib.rs` | metered_verifier | move-bytecode-verifier, move-bytecode-verifier-meter, move-cli, move-package-alt-compilation, move-unit-test, sui-config, sui-framework, sui-move, sui-move-build, sui-protocol-config |
| `sui-futures` | SuiComponent | `crates/sui-futures` | `src/lib.rs` | future, service, stream, task |  |
| `sui-genesis-builder` | SuiComponent | `crates/sui-genesis-builder` | `src/lib.rs` | validator_info | move-binary-format, move-core-types, shared-crypto, sui-config, sui-execution, sui-framework, sui-framework-snapshot, sui-protocol-config, sui-types |
| `sui-graphql-e2e-tests` | Testing | `crates/sui-graphql-e2e-tests` | `src/lib.rs` |  | sui-graphql-rpc, sui-transactional-test-runner, telemetry-subscribers |
| `sui-graphql-rpc` | SuiComponent | `crates/sui-graphql-rpc` | `src/lib.rs` | commands, config, context_data, error, extensions, metrics, mutation, server, test_infra, types | bin-version, move-binary-format, move-bytecode-utils, move-core-types, move-disassembler, move-ir-types, mysten-metrics, mysten-network, shared-crypto, simulacrum |
| `sui-graphql-rpc-client` | SuiComponent | `crates/sui-graphql-rpc-client` | `src/lib.rs` | response, simple_client | sui-graphql-rpc-headers |
| `sui-graphql-rpc-headers` | SuiComponent | `crates/sui-graphql-rpc-headers` | `src/lib.rs` |  |  |
| `sui-http` | SuiComponent | `crates/sui-http` | `src/lib.rs` | body, config, connection_handler, connection_info, fuse, io, listener |  |
| `sui-indexer` | Indexer | `crates/sui-indexer` | `src/lib.rs` | apis, backfill, config, database, db, errors, handlers, indexer, indexer_reader, metrics | move-binary-format, move-bytecode-utils, move-core-types, mysten-metrics, simulacrum, sui-config, sui-core, sui-data-ingestion-core, sui-json, sui-json-rpc |
| `sui-indexer-alt` | Indexer | `crates/sui-indexer-alt` | `src/lib.rs` | args, benchmark, config | bin-version, sui-default-config, sui-indexer-alt-framework, sui-indexer-alt-metrics, sui-indexer-alt-schema, sui-protocol-config, sui-synthetic-ingestion, sui-types, telemetry-subscribers |
| `sui-indexer-alt-consistent-api` | Indexer | `crates/sui-indexer-alt-consistent-api` | `src/lib.rs` | proto |  |
| `sui-indexer-alt-consistent-store` | Indexer | `crates/sui-indexer-alt-consistent-store` | `src/lib.rs` | args, config, db, indexer, metrics, restore, rpc, store | bin-version, move-core-types, mysten-network, sui-default-config, sui-futures, sui-indexer-alt-consistent-api, sui-indexer-alt-framework, sui-indexer-alt-metrics, sui-storage, telemetry-subscribers |
| `sui-indexer-alt-e2e-tests` | Indexer | `crates/sui-indexer-alt-e2e-tests` | `src/lib.rs` | coin_registry, find | move-core-types, shared-crypto, simulacrum, sui-field-count, sui-futures, sui-indexer-alt, sui-indexer-alt-consistent-api, sui-indexer-alt-consistent-store, sui-indexer-alt-framework, sui-indexer-alt-graphql |
| `sui-indexer-alt-framework` | Indexer | `crates/sui-indexer-alt-framework` | `src/lib.rs` | cluster, ingestion, metrics, pipeline, postgres, mocks | sui-field-count, sui-futures, sui-indexer-alt-framework-store-traits, sui-indexer-alt-metrics, sui-pg-db, sui-rpc-api, sui-storage, sui-synthetic-ingestion, sui-types, telemetry-subscribers |
| `sui-indexer-alt-framework-store-traits` | Indexer | `crates/sui-indexer-alt-framework-store-traits` | `src/lib.rs` |  |  |
| `sui-indexer-alt-graphql` | Indexer | `crates/sui-indexer-alt-graphql` | `src/lib.rs` | api, args, config, error, extensions, health, intersect, metrics, middleware, pagination | bin-version, move-binary-format, move-core-types, move-disassembler, move-ir-types, shared-crypto, sui-default-config, sui-display, sui-futures, sui-indexer-alt-metrics |
| `sui-indexer-alt-jsonrpc` | Indexer | `crates/sui-indexer-alt-jsonrpc` | `src/lib.rs` | api, args, config, context, data, error, metrics, paginate, timeout | bin-version, move-binary-format, move-core-types, sui-default-config, sui-display, sui-futures, sui-indexer-alt-metrics, sui-indexer-alt-reader, sui-indexer-alt-schema, sui-json |
| `sui-indexer-alt-metrics` | Indexer | `crates/sui-indexer-alt-metrics` | `src/lib.rs` | db | prometheus-closure-metric, sui-futures, sui-pg-db |
| `sui-indexer-alt-object-store` | Indexer | `crates/sui-indexer-alt-object-store` | `src/lib.rs` |  | sui-indexer-alt-framework-store-traits |
| `sui-indexer-alt-reader` | Indexer | `crates/sui-indexer-alt-reader` | `src/lib.rs` | bigtable_reader, checkpoints, coin_metadata, consistent_reader, cp_sequence_numbers, displays, epochs, error, events, fullnode_client | move-core-types, sui-futures, sui-indexer-alt-consistent-api, sui-indexer-alt-metrics, sui-indexer-alt-schema, sui-kvstore, sui-package-resolver, sui-pg-db, sui-rpc-api, sui-sql-macro |
| `sui-indexer-alt-restorer` | Indexer | `crates/sui-indexer-alt-restorer` | `src/lib.rs` | archives, snapshot | sui-config, sui-core, sui-data-ingestion-core, sui-field-count, sui-futures, sui-indexer-alt-schema, sui-pg-db, sui-snapshot, sui-storage, sui-types |
| `sui-indexer-alt-schema` | Indexer | `crates/sui-indexer-alt-schema` | `src/lib.rs` | checkpoints, cp_sequence_numbers, displays, epochs, events, objects, packages, schema, transactions | sui-field-count, sui-protocol-config, sui-types |
| `sui-indexer-builder` | Indexer | `crates/sui-indexer-builder` | `src/lib.rs` | indexer_builder, metrics, progress, sui_datasource | mysten-metrics, sui-data-ingestion-core, sui-indexer-builder, sui-sdk, sui-types, telemetry-subscribers |
| `sui-json` | SuiComponent | `crates/sui-json` | `src/lib.rs` | tests | move-binary-format, move-bytecode-utils, move-core-types, sui-framework, sui-move-build, sui-types |
| `sui-json-rpc` | Core | `crates/sui-json-rpc` | `src/lib.rs` | authority_state, balance_changes, bridge_api, coin_api, error, governance_api, indexer_api, logger, metrics, move_utils | move-binary-format, move-bytecode-utils, move-core-types, mysten-metrics, shared-crypto, sui-config, sui-core, sui-display, sui-json, sui-json-rpc-api |
| `sui-json-rpc-api` | Core | `crates/sui-json-rpc-api` | `src/lib.rs` | bridge, coin, extended, governance, indexer, move_utils, read, transaction_builder, write | mysten-metrics, sui-json, sui-json-rpc-types, sui-open-rpc, sui-open-rpc-macros, sui-types |
| `sui-json-rpc-tests` | RPC | `crates/sui-json-rpc-tests` | `` |  | move-core-types, shared-crypto, sui-config, sui-core, sui-json, sui-json-rpc, sui-json-rpc-api, sui-json-rpc-types, sui-keys, sui-macros |
| `sui-json-rpc-types` | Core | `crates/sui-json-rpc-types` | `src/lib.rs` | rpc_types_tests, balance_changes, displays, object_changes, sui_checkpoint, sui_coin, sui_event, sui_extended, sui_governance, sui_move | move-binary-format, move-bytecode-utils, move-command-line-common, move-core-types, move-disassembler, move-ir-types, mysten-metrics, sui-enum-compat-util, sui-json, sui-macros |
| `sui-keys` | SuiComponent | `crates/sui-keys` | `src/lib.rs` | external, key_derive, key_identity, keypair_file, keystore, random_names | jsonrpc, shared-crypto, sui-types |
| `sui-kv-rpc` | SuiComponent | `crates/sui-kv-rpc` | `src/lib.rs` | v2 | bin-version, mysten-metrics, mysten-network, sui-data-ingestion-core, sui-kvstore, sui-protocol-config, sui-rpc-api, sui-types, telemetry-subscribers |
| `sui-kvstore` | SuiComponent | `crates/sui-kvstore` | `src/lib.rs` | bigtable | sui-data-ingestion-core, sui-types, telemetry-subscribers |
| `sui-light-client` | SuiComponent | `crates/sui-light-client` | `src/lib.rs` | proof, checkpoint, config, object_store, package_store, graphql, mmr, verifier | move-binary-format, move-core-types, sui-config, sui-data-ingestion-core, sui-json-rpc-types, sui-package-resolver, sui-rpc-api, sui-sdk, sui-storage, sui-types |
| `sui-macros` | SuiComponent | `crates/sui-macros` | `src/lib.rs` |  | sui-proc-macros |
| `sui-metric-checker` | SuiComponent | `crates/sui-metric-checker` | `src/lib.rs` | query | telemetry-subscribers |
| `sui-metrics-push-client` | SuiComponent | `crates/sui-metrics-push-client` | `src/lib.rs` | client | mysten-metrics, sui-tls, sui-types |
| `sui-move` | SuiComponent | `crates/sui-move` | `src/lib.rs` | build, cache_package, coverage, disassemble, migrate, new, summary, unit_test, update_deps | bin-version, move-binary-format, move-bytecode-source-map, move-cli, move-compiler, move-core-types, move-disassembler, move-ir-types, move-package-alt, move-package-alt-compilation |
| `sui-move-build` | SuiComponent | `crates/sui-move-build` | `src/lib.rs` | build_tests | move-binary-format, move-bytecode-utils, move-bytecode-verifier, move-command-line-common, move-compiler, move-core-types, move-ir-types, move-package-alt, move-package-alt-compilation, move-symbol-pool |
| `sui-move-lsp` | SuiComponent | `crates/sui-move-lsp` | `` |  | bin-version, move-analyzer, move-compiler, sui-move-build, sui-package-alt, sui-package-management |
| `sui-name-service` | SuiComponent | `crates/sui-name-service` | `src/lib.rs` |  | move-core-types, sui-types |
| `sui-network` | Core | `crates/sui-network` | `src/lib.rs` | api, discovery, randomness, state_sync, utils, validator | mysten-common, mysten-metrics, mysten-network, shared-crypto, sui-config, sui-data-ingestion-core, sui-http, sui-macros, sui-protocol-config, sui-simulator |
| `sui-node` | Core | `crates/sui-node` | `src/lib.rs` | admin, handle, metrics | bin-version, consensus-core, move-vm-config, mysten-common, mysten-metrics, mysten-network, mysten-service, sui-config, sui-core, sui-http |
| `sui-open-rpc` | SuiComponent | `crates/sui-open-rpc` | `src/lib.rs` |  | move-core-types, sui-json, sui-json-rpc, sui-json-rpc-api, sui-json-rpc-types, sui-protocol-config, sui-types |
| `sui-open-rpc-macros` | SuiComponent | `crates/sui-open-rpc-macros` | `src/lib.rs` |  |  |
| `sui-oracle` | SuiComponent | `crates/sui-oracle` | `src/lib.rs` | config, metrics | mysten-metrics, shared-crypto, sui-config, sui-json-rpc-types, sui-keys, sui-move-build, sui-sdk, sui-types, telemetry-subscribers |
| `sui-package-alt` | SuiComponent | `crates/sui-package-alt` | `src/lib.rs` | environments, find_env, sui_flavor | bin-version, move-compiler, move-core-types, move-package-alt, move-package-alt-compilation, shared-crypto, sui-config, sui-json-rpc-types, sui-keys, sui-package-management |
| `sui-package-dump` | SuiComponent | `crates/sui-package-dump` | `src/lib.rs` | client, query | move-core-types, sui-types |
| `sui-package-management` | SuiComponent | `crates/sui-package-management` | `src/lib.rs` | system_package_versions | move-core-types, move-symbol-pool, sui-framework-snapshot, sui-json-rpc-types, sui-protocol-config, sui-sdk, sui-types |
| `sui-package-resolver` | SuiComponent | `crates/sui-package-resolver` | `src/lib.rs` | error | move-binary-format, move-command-line-common, move-compiler, move-core-types, sui-move-build, sui-types |
| `sui-pg-db` | SuiComponent | `crates/sui-pg-db` | `src/lib.rs` | model, tls, query, schema, store, temp | sui-field-count, sui-indexer-alt-framework-store-traits, sui-sql-macro, telemetry-subscribers |
| `sui-proc-macros` | SuiComponent | `crates/sui-proc-macros` | `src/lib.rs` |  | sui-enum-compat-util |
| `sui-protocol-config` | Core | `crates/sui-protocol-config` | `src/lib.rs` |  | move-binary-format, move-core-types, move-vm-config, sui-protocol-config-macros |
| `sui-protocol-config-macros` | SuiComponent | `crates/sui-protocol-config-macros` | `src/lib.rs` |  |  |
| `sui-proxy` | SuiComponent | `crates/sui-proxy` | `src/lib.rs` | admin, config, consumer, handlers, histogram_relay, metrics, middleware, peers, prom_to_mimir, remote_write | bin-version, mysten-metrics, sui-tls, sui-types, telemetry-subscribers |
| `sui-replay` | SuiComponent | `crates/sui-replay` | `src/lib.rs` | batch_replay, config, data_fetcher, displays, fuzz, fuzz_mutations, replay, tests, transaction_provider, types | move-binary-format, move-bytecode-utils, move-core-types, move-vm-config, shared-crypto, sui-config, sui-core, sui-execution, sui-framework, sui-json-rpc-api |
| `sui-replay-2` | SuiComponent | `crates/sui-replay-2` | `src/lib.rs` | artifacts, displays, execution, package_tools, replay_txn, summary_metrics, tracing | bin-version, move-binary-format, move-bytecode-source-map, move-cli, move-command-line-common, move-core-types, move-disassembler, move-ir-types, move-package-alt, move-package-alt-compilation |
| `sui-rosetta` | SuiComponent | `crates/sui-rosetta` | `src/lib.rs` | account, block, construction, errors, network, operations, state, types | move-cli, move-core-types, mysten-metrics, shared-crypto, sui-config, sui-keys, sui-move-build, sui-sdk, sui-swarm-config, sui-test-transaction-builder |
| `sui-rpc-api` | SuiComponent | `crates/sui-rpc-api` | `src/lib.rs` | client, config, error, grpc, metrics, reader, response, service, subscription | move-binary-format, move-core-types, mysten-network, sui-config, sui-name-service, sui-package-resolver, sui-protocol-config, sui-types |
| `sui-rpc-benchmark` | SuiComponent | `crates/sui-rpc-benchmark` | `src/lib.rs` | config, direct, json_rpc | sui-futures, telemetry-subscribers |
| `sui-rpc-loadgen` | SuiComponent | `crates/sui-rpc-loadgen` | `src/main.rs` | load_test, payload | shared-crypto, sui-json-rpc-types, sui-keys, sui-sdk, sui-types, telemetry-subscribers |
| `sui-rpc-resolver` | SuiComponent | `crates/sui-rpc-resolver` | `src/lib.rs` | json_visitor, package_store | move-core-types, sui-package-resolver, sui-rpc-api, sui-types |
| `sui-sdk` | SuiComponent | `crates/sui-sdk` | `src/lib.rs` | apis, error, json_rpc_error, sui_client_config, verify_personal_message_signature, wallet_context | move-core-types, mysten-common, shared-crypto, sui-config, sui-json, sui-json-rpc-api, sui-json-rpc-types, sui-keys, sui-macros, sui-protocol-config |
| `sui-security-watchdog` | SuiComponent | `crates/sui-security-watchdog` | `src/lib.rs` | metrics, pagerduty, query_runner, scheduler | mysten-metrics, telemetry-subscribers |
| `sui-simulator` | Testing | `crates/sui-simulator` | `src/lib.rs` |  | move-package-alt, move-package-alt-compilation, mysten-network, sui-framework, sui-move-build, sui-types, telemetry-subscribers |
| `sui-single-node-benchmark` | SuiComponent | `crates/sui-single-node-benchmark` | `src/lib.rs` | command, workload | move-binary-format, move-bytecode-utils, move-core-types, move-symbol-pool, sui-config, sui-core, sui-macros, sui-move-build, sui-protocol-config, sui-simulator |
| `sui-snapshot` | SuiComponent | `crates/sui-snapshot` | `src/lib.rs` | tests, reader, uploader, writer | sui-config, sui-core, sui-futures, sui-protocol-config, sui-storage, sui-types |
| `sui-source-validation` | SuiComponent | `crates/sui-source-validation` | `src/lib.rs` | error, toolchain, tests | move-binary-format, move-bytecode-source-map, move-command-line-common, move-compiler, move-core-types, move-package-alt, move-package-alt-compilation, move-symbol-pool, sui-config, sui-json-rpc-types |
| `sui-sql-macro` | SuiComponent | `crates/sui-sql-macro` | `src/lib.rs` | lexer, parser |  |
| `sui-storage` | Core | `crates/sui-storage` | `src/lib.rs` | blob, http_key_value_store, key_value_store, key_value_store_metrics, mutex_table, object_store, package_object_cache, sharded_lru, write_path_pending_tx_log | move-binary-format, move-bytecode-utils, move-core-types, mysten-metrics, sui-config, sui-json-rpc-types, sui-macros, sui-protocol-config, sui-simulator, sui-test-transaction-builder |
| `sui-surfer` | SuiComponent | `crates/sui-surfer` | `src/lib.rs` | surf_strategy, surfer_state, surfer_task | move-binary-format, move-core-types, mysten-common, sui-core, sui-json-rpc-types, sui-macros, sui-move-build, sui-protocol-config, sui-simulator, sui-swarm-config |
| `sui-swarm` | SuiComponent | `crates/sui-swarm` | `src/lib.rs` | memory | mysten-metrics, mysten-network, sui-config, sui-macros, sui-node, sui-protocol-config, sui-swarm-config, sui-tls, sui-types, telemetry-subscribers |
| `sui-swarm-config` | SuiComponent | `crates/sui-swarm-config` | `src/lib.rs` | genesis_config, network_config, network_config_builder, node_config_builder, test_utils | move-bytecode-utils, mysten-common, shared-crypto, sui-config, sui-execution, sui-genesis-builder, sui-macros, sui-protocol-config, sui-rpc-api, sui-types |
| `sui-synthetic-ingestion` | SuiComponent | `crates/sui-synthetic-ingestion` | `src/lib.rs` | synthetic_ingestion | simulacrum, sui-storage, sui-test-transaction-builder, sui-types, telemetry-subscribers |
| `sui-telemetry` | SuiComponent | `crates/sui-telemetry` | `src/lib.rs` |  | sui-core |
| `sui-test-transaction-builder` | Testing | `crates/sui-test-transaction-builder` | `src/lib.rs` |  | move-core-types, shared-crypto, sui-genesis-builder, sui-move-build, sui-sdk, sui-types |
| `sui-test-validator` | Testing | `crates/sui-test-validator` | `src/main.rs` |  |  |
| `sui-tls` | SuiComponent | `crates/sui-tls` | `src/lib.rs` | acceptor, certgen, verifier |  |
| `sui-tool` | SuiComponent | `crates/sui-tool` | `src/lib.rs` | commands, db_tool, formal_snapshot_util | bin-version, consensus-core, move-core-types, mysten-metrics, sui-config, sui-core, sui-data-ingestion-core, sui-network, sui-package-dump, sui-protocol-config |
| `sui-transaction-builder` | SuiComponent | `crates/sui-transaction-builder` | `src/lib.rs` |  | move-binary-format, move-core-types, sui-json, sui-json-rpc-types, sui-protocol-config, sui-types |
| `sui-transaction-checks` | SuiComponent | `crates/sui-transaction-checks` | `src/lib.rs` | deny | sui-config, sui-execution, sui-macros, sui-protocol-config, sui-types |
| `sui-transactional-test-runner` | Testing | `crates/sui-transactional-test-runner` | `src/lib.rs` | args, cursor, offchain_state, programmable_transaction_test_parser, simulator_persisted_store, test_adapter | move-binary-format, move-bytecode-utils, move-command-line-common, move-compiler, move-core-types, move-stdlib, move-symbol-pool, move-transactional-test-runner, move-vm-runtime, simulacrum |
| `sui-types` | Core | `crates/sui-types` | `src/lib.rs` | error, accumulator_event, accumulator_metadata, accumulator_root, address_alias, authenticator_state, balance, balance_change, base_types, bridge | consensus-config, consensus-types, move-binary-format, move-bytecode-utils, move-core-types, move-trace-format, move-vm-profiler, move-vm-test-utils, mysten-common, mysten-metrics |
| `sui-upgrade-compatibility-transactional-tests` | Testing | `crates/sui-upgrade-compatibility-transactional-tests` | `src/lib.rs` |  | move-binary-format, move-command-line-common, move-compiler, sui-move-build |
| `sui-verifier-transactional-tests` | Testing | `crates/sui-verifier-transactional-tests` | `src/lib.rs` |  | sui-transactional-test-runner |
