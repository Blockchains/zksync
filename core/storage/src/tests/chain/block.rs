// External imports
use web3::types::H256;
// Workspace imports
use crypto_exports::{ff::PrimeField, rand::XorShiftRng};
use models::node::{apply_updates, block::Block, AccountMap, AccountUpdate, BlockNumber, Fr};
use models::{ethereum::OperationType, fe_to_bytes, Action, Operation};
// Local imports
use super::utils::{acc_create_random_updates, get_operation, get_operation_with_txs};
use crate::tests::{create_rng, db_test};
use crate::{
    chain::{
        block::{records::BlockDetails, BlockSchema},
        state::StateSchema,
    },
    ethereum::EthereumSchema,
    prover::ProverSchema,
    StorageProcessor,
};

/// block size used for this tests
const BLOCK_SIZE_CHUNKS: usize = 100;

/// Creates several random updates for the provided account map,
/// and returns the resulting account map together with the list
/// of generated updates.
pub fn apply_random_updates(
    mut accounts: AccountMap,
    rng: &mut XorShiftRng,
) -> (AccountMap, Vec<(u32, AccountUpdate)>) {
    let updates = {
        let mut updates = Vec::new();
        updates.extend(acc_create_random_updates(rng));
        updates.extend(acc_create_random_updates(rng));
        updates.extend(acc_create_random_updates(rng));
        updates
    };
    apply_updates(&mut accounts, updates.clone());
    (accounts, updates)
}

/// Here we create updates for blocks 1,2,3 (commit 3 blocks)
/// We apply updates for blocks 1,2 (verify 2 blocks)
/// Make sure that we can get state for all blocks.
#[test]
#[cfg_attr(not(feature = "db_test"), ignore)]
fn test_commit_rewind() {
    let _ = env_logger::try_init();
    let mut rng = create_rng();

    let conn = StorageProcessor::establish_connection().unwrap();
    db_test(conn.conn(), || {
        // Create the input data for three blocks.
        // Data for the next block is based on previous block data.
        let (accounts_block_1, updates_block_1) =
            apply_random_updates(AccountMap::default(), &mut rng);
        let (accounts_block_2, updates_block_2) =
            apply_random_updates(accounts_block_1.clone(), &mut rng);
        let (accounts_block_3, updates_block_3) =
            apply_random_updates(accounts_block_2.clone(), &mut rng);

        // Execute and commit these blocks.
        BlockSchema(&conn).execute_operation(get_operation(
            1,
            Action::Commit,
            updates_block_1,
            BLOCK_SIZE_CHUNKS,
        ))?;
        BlockSchema(&conn).execute_operation(get_operation(
            2,
            Action::Commit,
            updates_block_2,
            BLOCK_SIZE_CHUNKS,
        ))?;
        BlockSchema(&conn).execute_operation(get_operation(
            3,
            Action::Commit,
            updates_block_3,
            BLOCK_SIZE_CHUNKS,
        ))?;

        // Check that they are stored in state.
        let (block, state) = StateSchema(&conn).load_committed_state(Some(1)).unwrap();
        assert_eq!((block, &state), (1, &accounts_block_1));

        let (block, state) = StateSchema(&conn).load_committed_state(Some(2)).unwrap();
        assert_eq!((block, &state), (2, &accounts_block_2));

        let (block, state) = StateSchema(&conn).load_committed_state(Some(3)).unwrap();
        assert_eq!((block, &state), (3, &accounts_block_3));

        // Add proofs for the first two blocks.
        ProverSchema(&conn).store_proof(1, &Default::default())?;
        BlockSchema(&conn).execute_operation(get_operation(
            1,
            Action::Verify {
                proof: Default::default(),
            },
            Vec::new(),
            BLOCK_SIZE_CHUNKS,
        ))?;
        ProverSchema(&conn).store_proof(2, &Default::default())?;
        BlockSchema(&conn).execute_operation(get_operation(
            2,
            Action::Verify {
                proof: Default::default(),
            },
            Vec::new(),
            BLOCK_SIZE_CHUNKS,
        ))?;

        // Check that we still can get the state for these blocks.
        let (block, state) = StateSchema(&conn).load_committed_state(Some(1)).unwrap();
        assert_eq!((block, &state), (1, &accounts_block_1));

        let (block, state) = StateSchema(&conn).load_committed_state(Some(2)).unwrap();
        assert_eq!((block, &state), (2, &accounts_block_2));

        let (block, state) = StateSchema(&conn).load_committed_state(Some(3)).unwrap();
        assert_eq!((block, &state), (3, &accounts_block_3));

        // Check that with no id provided, the latest state is loaded.
        let (block, state) = StateSchema(&conn).load_committed_state(None).unwrap();
        assert_eq!((block, &state), (3, &accounts_block_3));

        Ok(())
    });
}

/// Creates an unique new root hash for the block based on its number.
fn root_hash_for_block(block_number: BlockNumber) -> Fr {
    Fr::from_str(&block_number.to_string()).unwrap()
}

/// Creates an unique ethereum operation hash based on its number.
fn ethereum_tx_hash(ethereum_op_id: i64) -> H256 {
    H256::from_low_u64_ne(ethereum_op_id as u64)
}

/// Creates an operation with an unique hash.
fn get_unique_operation(
    block_number: BlockNumber,
    action: Action,
    accounts_updated: Vec<(u32, AccountUpdate)>,
) -> Operation {
    Operation {
        id: None,
        action,
        block: Block::new(
            block_number,
            root_hash_for_block(block_number),
            0,
            Vec::new(),
            (0, 0),
            100,
            1_000_000.into(),
            1_500_000.into(),
        ),
        accounts_updated,
    }
}

/// Checks that `find_block_by_height_or_hash` method allows
/// to load the block details by either its height, hash of the included
/// transaction, or the root hash of the block.
#[test]
#[cfg_attr(not(feature = "db_test"), ignore)]
fn find_block_by_height_or_hash() {
    /// The actual test check. It obtains the block details using
    /// the `find_block_by_height_or_hash` method with different types of query,
    /// and compares them against the provided sample.
    fn check_find_block_by_height_or_hash(
        conn: &StorageProcessor,
        expected_block_detail: &BlockDetails,
    ) -> diesel::QueryResult<()> {
        let mut queries = vec![
            expected_block_detail.block_number.to_string(),
            hex::encode(&expected_block_detail.new_state_root),
            hex::encode(&expected_block_detail.commit_tx_hash.as_ref().unwrap()),
        ];
        if let Some(verify_tx_hash) = expected_block_detail.verify_tx_hash.as_ref() {
            queries.push(hex::encode(&verify_tx_hash));
        }

        for query in queries {
            let actual_block_detail = BlockSchema(&conn)
                .find_block_by_height_or_hash(query.clone())
                .unwrap_or_else(|| {
                    panic!(format!(
                        "Can't load the existing block with the index {} using query {}",
                        expected_block_detail.block_number, query
                    ))
                });
            assert_eq!(
                actual_block_detail.block_number,
                expected_block_detail.block_number
            );
            assert_eq!(
                actual_block_detail.new_state_root,
                expected_block_detail.new_state_root
            );
            assert_eq!(
                actual_block_detail.commit_tx_hash,
                expected_block_detail.commit_tx_hash
            );
            assert_eq!(
                actual_block_detail.verify_tx_hash,
                expected_block_detail.verify_tx_hash
            );
        }

        Ok(())
    }

    // Below the initialization of the data for the test and collecting
    // the reference block detail samples.

    let mut rng = create_rng();

    let conn = StorageProcessor::establish_connection().unwrap();
    db_test(conn.conn(), || {
        // Required since we use `EthereumSchema` in this test.
        EthereumSchema(&conn).initialize_eth_data()?;

        let mut accounts_map = AccountMap::default();
        let n_committed = 5;
        let n_verified = n_committed - 2;

        let mut expected_outcome: Vec<BlockDetails> = Vec::new();

        // Create and apply several blocks to work with.
        for block_number in 1..=n_committed {
            // Create blanked block detail object which we will fill
            // with the relevant data and use for the comparison later.
            let mut current_block_detail = BlockDetails {
                block_number: 0,
                new_state_root: Default::default(),
                block_size: 0,
                commit_tx_hash: None,
                verify_tx_hash: None,
                committed_at: chrono::NaiveDateTime::from_timestamp(0, 0),
                verified_at: None,
            };

            let (new_accounts_map, updates) = apply_random_updates(accounts_map.clone(), &mut rng);
            accounts_map = new_accounts_map;

            // Store the operation in the block schema.
            let operation = BlockSchema(&conn).execute_operation(get_unique_operation(
                block_number,
                Action::Commit,
                updates,
            ))?;

            // Store & confirm the operation in the ethereum schema, as it's used for obtaining
            // commit/verify hashes.
            let ethereum_op_id = operation.id.unwrap() as i64;
            let eth_tx_hash = ethereum_tx_hash(ethereum_op_id);
            let response = EthereumSchema(&conn).save_new_eth_tx(
                OperationType::Commit,
                Some(ethereum_op_id),
                100,
                100u32.into(),
                Default::default(),
            )?;
            EthereumSchema(&conn).add_hash_entry(response.id, &eth_tx_hash)?;
            EthereumSchema(&conn).confirm_eth_tx(&eth_tx_hash)?;

            // Initialize reference sample fields.
            current_block_detail.block_number = operation.block.block_number as i64;
            current_block_detail.new_state_root = fe_to_bytes(&operation.block.new_root_hash);
            current_block_detail.block_size = operation.block.block_transactions.len() as i64;
            current_block_detail.commit_tx_hash = Some(eth_tx_hash.as_ref().to_vec());

            // Add verification for the block if required.
            if block_number <= n_verified {
                ProverSchema(&conn).store_proof(block_number, &Default::default())?;
                let verify_operation =
                    BlockSchema(&conn).execute_operation(get_unique_operation(
                        block_number,
                        Action::Verify {
                            proof: Default::default(),
                        },
                        Vec::new(),
                    ))?;

                let ethereum_op_id = verify_operation.id.unwrap() as i64;
                let eth_tx_hash = ethereum_tx_hash(ethereum_op_id);

                // Do not add an ethereum confirmation for the last operation.
                if block_number != n_verified {
                    let response = EthereumSchema(&conn).save_new_eth_tx(
                        OperationType::Verify,
                        Some(ethereum_op_id),
                        100,
                        100u32.into(),
                        Default::default(),
                    )?;
                    EthereumSchema(&conn).add_hash_entry(response.id, &eth_tx_hash)?;
                    EthereumSchema(&conn).confirm_eth_tx(&eth_tx_hash)?;
                    current_block_detail.verify_tx_hash = Some(eth_tx_hash.as_ref().to_vec());
                }
            }

            // Store the sample.
            expected_outcome.push(current_block_detail);
        }

        // Run the tests against the collected data.
        for expected_block_detail in expected_outcome {
            check_find_block_by_height_or_hash(&conn, &expected_block_detail)?;
        }

        // Also check that we get `None` for non-existing block.
        let query = 10000.to_string();
        assert!(BlockSchema(&conn)
            .find_block_by_height_or_hash(query)
            .is_none());

        Ok(())
    });
}

/// Checks that `load_block_range` method loads the range of blocks correctly.
#[test]
#[cfg_attr(not(feature = "db_test"), ignore)]
fn block_range() {
    /// Loads the block range and checks that every block in the response is
    /// equal to the one obtained from `find_block_by_height_or_hash` method.
    fn check_block_range(
        conn: &StorageProcessor,
        max_block: BlockNumber,
        limit: u32,
    ) -> diesel::QueryResult<()> {
        let start_block = if max_block >= limit {
            (max_block - limit) + 1
        } else {
            1
        };
        let block_range = BlockSchema(conn).load_block_range(max_block, limit)?;
        // Go in the reversed order, since the blocks themselves are ordered backwards.
        for (idx, block_number) in (start_block..=max_block).rev().enumerate() {
            let expected = BlockSchema(&conn)
                .find_block_by_height_or_hash(block_number.to_string())
                .unwrap_or_else(|| {
                    panic!(format!(
                        "Can't load the existing block with the index {}",
                        block_number
                    ))
                });
            let got = &block_range[idx];
            assert_eq!(got, &expected);
        }

        Ok(())
    }

    // Below lies the initialization of the data for the test.

    let mut rng = create_rng();

    let conn = StorageProcessor::establish_connection().unwrap();
    db_test(conn.conn(), || {
        // Required since we use `EthereumSchema` in this test.
        EthereumSchema(&conn).initialize_eth_data()?;

        let mut accounts_map = AccountMap::default();
        let n_committed = 5;
        let n_verified = n_committed - 2;

        // Create and apply several blocks to work with.
        for block_number in 1..=n_committed {
            let (new_accounts_map, updates) = apply_random_updates(accounts_map.clone(), &mut rng);
            accounts_map = new_accounts_map;

            // Store the operation in the block schema.
            let operation = BlockSchema(&conn).execute_operation(get_unique_operation(
                block_number,
                Action::Commit,
                updates,
            ))?;

            // Store & confirm the operation in the ethereum schema, as it's used for obtaining
            // commit/verify hashes.
            let ethereum_op_id = operation.id.unwrap() as i64;
            let eth_tx_hash = ethereum_tx_hash(ethereum_op_id);
            let response = EthereumSchema(&conn).save_new_eth_tx(
                OperationType::Commit,
                Some(ethereum_op_id),
                100,
                100u32.into(),
                Default::default(),
            )?;
            EthereumSchema(&conn).add_hash_entry(response.id, &eth_tx_hash)?;

            // Add verification for the block if required.
            if block_number <= n_verified {
                ProverSchema(&conn).store_proof(block_number, &Default::default())?;
                let operation = BlockSchema(&conn).execute_operation(get_unique_operation(
                    block_number,
                    Action::Verify {
                        proof: Default::default(),
                    },
                    Vec::new(),
                ))?;
                let ethereum_op_id = operation.id.unwrap() as i64;
                let eth_tx_hash = ethereum_tx_hash(ethereum_op_id);
                let response = EthereumSchema(&conn).save_new_eth_tx(
                    OperationType::Verify,
                    Some(ethereum_op_id),
                    100,
                    100u32.into(),
                    Default::default(),
                )?;
                EthereumSchema(&conn).add_hash_entry(response.id, &eth_tx_hash)?;
                EthereumSchema(&conn).confirm_eth_tx(&eth_tx_hash)?;
            }
        }

        // Check the block range method given the various combinations of the limit and the end block.
        let test_vector = vec![
            (n_committed as BlockNumber, n_committed),
            (n_verified as BlockNumber, n_verified),
            (n_committed as BlockNumber, n_verified),
            (n_verified as BlockNumber, 1),
            (n_committed as BlockNumber, 1),
            (n_committed as BlockNumber, 0),
            (n_committed as BlockNumber, 100),
        ];

        for (max_block, limit) in test_vector {
            check_block_range(&conn, max_block, limit)?;
        }

        Ok(())
    });
}

/// Tests for the `load_commits_after_block` method.
#[test]
#[cfg_attr(not(feature = "db_test"), ignore)]
fn load_commits_after_block() {
    let _ = env_logger::try_init();
    let mut rng = create_rng();

    let conn = StorageProcessor::establish_connection().unwrap();
    db_test(conn.conn(), || {
        // Create the input data for three blocks.
        // Data for the next block is based on previous block data.
        let mut operations = Vec::new();
        let mut accounts = AccountMap::default();
        for block_id in 1..=3 {
            let (new_accounts, updates) = apply_random_updates(accounts.clone(), &mut rng);
            accounts = new_accounts;
            let operation = BlockSchema(&conn).execute_operation(get_operation(
                block_id,
                Action::Commit,
                updates,
                BLOCK_SIZE_CHUNKS,
            ))?;

            operations.push(operation);
        }

        // Add proofs for the first block.
        ProverSchema(&conn).store_proof(1, &Default::default())?;
        BlockSchema(&conn).execute_operation(get_operation(
            1,
            Action::Verify {
                proof: Default::default(),
            },
            Vec::new(),
            BLOCK_SIZE_CHUNKS,
        ))?;
        ProverSchema(&conn).store_proof(3, &Default::default())?;

        // Now test the method.
        let empty_vec = vec![];
        let test_vector = vec![
            // Blocks 2 & 3.
            ((1, 2), &operations[1..3], vec![false, true]),
            // Block 2.
            ((1, 1), &operations[1..2], vec![false]),
            // Block 3.
            ((2, 1), &operations[2..3], vec![true]),
            // No block (there are no blocks AFTER block 3.
            ((3, 1), &empty_vec, vec![]),
            // Obviously none.
            ((4, 100), &empty_vec, vec![]),
        ];

        for ((block, limit), expected_slice, has_proof) in test_vector {
            let commits = BlockSchema(&conn).load_commits_after_block(block, limit)?;

            assert_eq!(commits.len(), expected_slice.len());

            for ((expected, (got, got_hash_proof)), expect_hash_proof) in
                expected_slice.iter().zip(commits).zip(has_proof)
            {
                assert_eq!(expected.id, got.id);
                assert_eq!(expect_hash_proof, got_hash_proof);
            }
        }

        Ok(())
    });
}

/// Checks the pending block workflow:
/// - Transactions from the pending block are available for getting.
/// - `load_pending_block` loads the block correctly.
/// - Committing the final block causes pending block to be removed.
#[test]
#[cfg_attr(not(feature = "db_test"), ignore)]
fn pending_block_workflow() {
    use crate::chain::operations_ext::OperationsExtSchema;
    use models::node::{
        block::PendingBlock,
        operations::{ChangePubKeyOp, TransferToNewOp},
        ExecutedOperations, ExecutedTx, FranklinOp, FranklinTx,
    };
    use testkit::zksync_account::ZksyncAccount;

    let _ = env_logger::try_init();

    let from_account_id = 0xbabe;
    let from_zksync_account = ZksyncAccount::rand();
    from_zksync_account.set_account_id(Some(from_account_id));

    let to_account_id = 0xdcba;
    let to_zksync_account = ZksyncAccount::rand();
    to_zksync_account.set_account_id(Some(to_account_id));

    let (tx_1, executed_tx_1) = {
        let tx = from_zksync_account.create_change_pubkey_tx(None, false, false);

        let change_pubkey_op = FranklinOp::ChangePubKeyOffchain(Box::new(ChangePubKeyOp {
            tx: tx.clone(),
            account_id: from_account_id,
        }));

        let executed_change_pubkey_op = ExecutedTx {
            signed_tx: change_pubkey_op.try_get_tx().unwrap().into(),
            success: true,
            op: Some(change_pubkey_op),
            fail_reason: None,
            block_index: None,
            created_at: chrono::Utc::now(),
        };

        (
            FranklinTx::ChangePubKey(Box::new(tx)),
            ExecutedOperations::Tx(Box::new(executed_change_pubkey_op)),
        )
    };
    let (tx_2, executed_tx_2) = {
        let tx = from_zksync_account
            .sign_transfer(
                0,
                "",
                1u32.into(),
                0u32.into(),
                &to_zksync_account.address,
                None,
                true,
            )
            .0;

        let transfer_to_new_op = FranklinOp::TransferToNew(Box::new(TransferToNewOp {
            tx: tx.clone(),
            from: from_account_id,
            to: to_account_id,
        }));

        let executed_transfer_to_new_op = ExecutedTx {
            signed_tx: transfer_to_new_op.try_get_tx().unwrap().into(),
            success: true,
            op: Some(transfer_to_new_op),
            fail_reason: None,
            block_index: None,
            created_at: chrono::Utc::now(),
        };

        (
            FranklinTx::Transfer(Box::new(tx)),
            ExecutedOperations::Tx(Box::new(executed_transfer_to_new_op)),
        )
    };

    let txs_1 = vec![executed_tx_1];
    let txs_2 = vec![executed_tx_2];

    let block_1 = get_operation_with_txs(
        1,
        Action::Commit,
        Default::default(),
        BLOCK_SIZE_CHUNKS,
        txs_1.clone(),
    );
    let block_2 = get_operation_with_txs(
        2,
        Action::Commit,
        Default::default(),
        BLOCK_SIZE_CHUNKS,
        txs_2.clone(),
    );

    let conn = StorageProcessor::establish_connection().unwrap();
    db_test(conn.conn(), || {
        let pending_block_1 = PendingBlock {
            number: 1,
            chunks_left: 10,
            unprocessed_priority_op_before: 0,
            pending_block_iteration: 1,
            success_operations: txs_1,
        };
        let pending_block_2 = PendingBlock {
            number: 2,
            chunks_left: 12,
            unprocessed_priority_op_before: 0,
            pending_block_iteration: 2,
            success_operations: txs_2,
        };

        // Save pending block
        BlockSchema(&conn).save_pending_block(pending_block_1.clone())?;

        // Load saved block and check its correctness.
        let pending_block = BlockSchema(&conn)
            .load_pending_block()?
            .expect("No pending block");
        assert_eq!(pending_block.number, pending_block_1.number);
        assert_eq!(pending_block.chunks_left, pending_block_1.chunks_left);
        assert_eq!(
            pending_block.unprocessed_priority_op_before,
            pending_block_1.unprocessed_priority_op_before
        );
        assert_eq!(
            pending_block.pending_block_iteration,
            pending_block_1.pending_block_iteration
        );
        assert_eq!(
            pending_block.success_operations.len(),
            pending_block_1.success_operations.len()
        );

        // Check that stored tx can already be loaded from the database.
        let pending_ops = BlockSchema(&conn).get_block_executed_ops(1)?;
        assert_eq!(pending_ops.len(), 1);

        // Also check that we can find the transaction by its hash.
        assert!(
            OperationsExtSchema(&conn)
                .get_tx_by_hash(&tx_1.hash().as_ref())?
                .is_some(),
            "Cannot find the pending transaction by hash"
        );

        // Finalize the block.
        BlockSchema(&conn).execute_operation(block_1)?;

        // Ensure that pending block is no more available.
        assert!(
            BlockSchema(&conn).load_pending_block()?.is_none(),
            "Pending block was not removed after commit"
        );

        // Repeat the checks with the second block. Now we'll check for
        // both committed (1st) and pending (2nd) blocks data to be available.
        BlockSchema(&conn).save_pending_block(pending_block_2.clone())?;

        let pending_block = BlockSchema(&conn)
            .load_pending_block()?
            .expect("No pending block");
        assert_eq!(pending_block.number, pending_block_2.number);

        // Check that stored tx can already be loaded from the database.
        let committed_ops = BlockSchema(&conn).get_block_executed_ops(1)?;
        assert_eq!(committed_ops.len(), 1);
        let pending_ops = BlockSchema(&conn).get_block_executed_ops(2)?;
        assert_eq!(pending_ops.len(), 1);

        // Also check that we can find the transaction by its hash.
        assert!(
            OperationsExtSchema(&conn)
                .get_tx_by_hash(&tx_1.hash().as_ref())?
                .is_some(),
            "Cannot find the pending transaction by hash"
        );
        assert!(
            OperationsExtSchema(&conn)
                .get_tx_by_hash(&tx_2.hash().as_ref())?
                .is_some(),
            "Cannot find the pending transaction by hash"
        );

        // Finalize the block.
        BlockSchema(&conn).execute_operation(block_2)?;

        // Ensure that pending block is no more available.
        assert!(
            BlockSchema(&conn).load_pending_block()?.is_none(),
            "Pending block was not removed after commit"
        );

        Ok(())
    });
}
/// Here we create blocks and publish proofs for them in different order
#[test]
#[cfg_attr(not(feature = "db_test"), ignore)]
fn test_unproven_block_query() {
    let _ = env_logger::try_init();

    let conn = StorageProcessor::establish_connection().unwrap();
    db_test(conn.conn(), || {
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 0);

        // Execute and commit these blocks.
        BlockSchema(&conn).execute_operation(get_operation(
            1,
            Action::Commit,
            Vec::new(),
            BLOCK_SIZE_CHUNKS,
        ))?;
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 1);
        BlockSchema(&conn).execute_operation(get_operation(
            2,
            Action::Commit,
            Vec::new(),
            BLOCK_SIZE_CHUNKS,
        ))?;
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 2);
        BlockSchema(&conn).execute_operation(get_operation(
            3,
            Action::Commit,
            Vec::new(),
            BLOCK_SIZE_CHUNKS,
        ))?;
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 3);

        // Add proofs for the first two blocks.
        ProverSchema(&conn).store_proof(3, &Default::default())?;
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 2);
        ProverSchema(&conn).store_proof(1, &Default::default())?;
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 1);
        BlockSchema(&conn).execute_operation(get_operation(
            1,
            Action::Verify {
                proof: Default::default(),
            },
            Vec::new(),
            BLOCK_SIZE_CHUNKS,
        ))?;
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 1);
        ProverSchema(&conn).store_proof(2, &Default::default())?;
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 0);
        BlockSchema(&conn).execute_operation(get_operation(
            2,
            Action::Verify {
                proof: Default::default(),
            },
            Vec::new(),
            BLOCK_SIZE_CHUNKS,
        ))?;
        assert_eq!(ProverSchema(&conn).pending_jobs_count()?, 0);
        Ok(())
    });
}
