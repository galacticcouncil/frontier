// SPDX-License-Identifier: Apache-2.0
// This file is part of Frontier.
//
// Copyright (c) 2020-2022 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use ethereum::{
	AccessList, AccessListItem, EIP1559Transaction, EIP2930Transaction, LegacyTransaction,
	TransactionAction, TransactionSignature, TransactionV2,
};
use ethereum_types::{H160, H256, U256};
use scale_info::TypeInfo;
use sp_std::vec::Vec;

#[derive(Clone, Debug, Eq, PartialEq, Encode, Decode, TypeInfo)]
/// Manually sets a gas fee.
pub struct ManualEthereumXcmFee {
	/// Legacy or Eip-2930
	pub gas_price: Option<U256>,
	/// Eip-1559
	pub max_fee_per_gas: Option<U256>,
	/// Eip-1559
	pub max_priority_fee_per_gas: Option<U256>,
}

#[derive(Clone, Debug, Eq, PartialEq, Encode, Decode, TypeInfo)]
/// Authomatic gas fee based on the current on-chain values.
/// Will always produce an Eip-1559 transaction.
pub enum AutoEthereumXcmFee {
	/// base_fee_per_gas = BaseFee
	Low,
	/// max_fee_per_gas = 2 * BaseFee, max_priority_fee_per_gas = BaseFee
	Medium,
	/// max_fee_per_gas = 3 * BaseFee, max_priority_fee_per_gas = 2 * BaseFee
	High,
}

/// Xcm transact's Ethereum transaction configurable fee.
#[derive(Clone, Debug, Eq, PartialEq, Encode, Decode, TypeInfo)]
pub enum EthereumXcmFee {
	Manual(ManualEthereumXcmFee),
	Auto(AutoEthereumXcmFee),
}

/// Xcm transact's Ethereum transaction.
#[derive(Clone, Debug, Eq, PartialEq, Encode, Decode, TypeInfo)]
pub enum EthereumXcmTransaction {
	V1(EthereumXcmTransactionV1),
}

/// Value for `r` and `s` for the invalid signature included in Xcm transact's Ethereum transaction.
pub fn rs_id() -> H256 {
	H256::from_low_u64_be(1u64)
}

#[derive(Clone, Debug, Eq, PartialEq, Encode, Decode, TypeInfo)]
pub struct EthereumXcmTransactionV1 {
	/// Gas limit to be consumed by EVM execution.
	pub gas_limit: U256,
	/// Fee configuration of choice.
	pub fee_payment: EthereumXcmFee,
	/// Either a Call (the callee, account or contract address) or Create (currently unsupported).
	pub action: TransactionAction,
	/// Value to be transfered.
	pub value: U256,
	/// Input data for a contract call.
	pub input: Vec<u8>,
	/// Map of addresses to be pre-paid to warm storage.
	pub access_list: Option<Vec<(H160, Vec<H256>)>>,
}

pub trait XcmToEthereum {
	fn into_transaction_v2(&self, base_fee: U256, nonce: U256) -> Option<TransactionV2>;
}

impl XcmToEthereum for EthereumXcmTransaction {
	fn into_transaction_v2(&self, base_fee: U256, nonce: U256) -> Option<TransactionV2> {
		match self {
			EthereumXcmTransaction::V1(v1_tx) => v1_tx.into_transaction_v2(base_fee, nonce),
		}
	}
}

impl XcmToEthereum for EthereumXcmTransactionV1 {
	fn into_transaction_v2(&self, base_fee: U256, nonce: U256) -> Option<TransactionV2> {
		let from_tuple_to_access_list = |t: &Vec<(H160, Vec<H256>)>| -> AccessList {
			t.iter()
				.map(|item| AccessListItem {
					address: item.0.clone(),
					storage_keys: item.1.clone(),
				})
				.collect::<Vec<AccessListItem>>()
		};

		let (gas_price, max_fee, max_priority_fee) = match &self.fee_payment {
			EthereumXcmFee::Manual(fee_config) => (
				fee_config.gas_price,
				fee_config.max_fee_per_gas,
				fee_config.max_priority_fee_per_gas,
			),
			EthereumXcmFee::Auto(auto_mode) => {
				let (max_fee, max_priority_fee) = match auto_mode {
					AutoEthereumXcmFee::Low => (Some(base_fee), None),
					AutoEthereumXcmFee::Medium => (
						Some(base_fee.saturating_mul(U256::from(2u64))),
						Some(base_fee),
					),
					AutoEthereumXcmFee::High => (
						Some(base_fee.saturating_mul(U256::from(3u64))),
						Some(base_fee.saturating_mul(U256::from(2u64))),
					),
				};
				(None, max_fee, max_priority_fee)
			}
		};
		match (gas_price, max_fee, max_priority_fee) {
			(Some(gas_price), None, None) => {
				// Legacy or Eip-2930
				if let Some(ref access_list) = self.access_list {
					// Eip-2930
					Some(TransactionV2::EIP2930(EIP2930Transaction {
						chain_id: 0,
						nonce,
						gas_price,
						gas_limit: self.gas_limit,
						action: self.action,
						value: self.value,
						input: self.input.clone(),
						access_list: from_tuple_to_access_list(access_list),
						odd_y_parity: true,
						r: rs_id(),
						s: rs_id(),
					}))
				} else {
					// Legacy
					Some(TransactionV2::Legacy(LegacyTransaction {
						nonce,
						gas_price,
						gas_limit: self.gas_limit,
						action: self.action,
						value: self.value,
						input: self.input.clone(),
						signature: TransactionSignature::new(42, rs_id(), rs_id()).unwrap(), // TODO
					}))
				}
			}
			(None, Some(max_fee), _) => {
				// Eip-1559
				Some(TransactionV2::EIP1559(EIP1559Transaction {
					chain_id: 0,
					nonce,
					max_fee_per_gas: max_fee,
					max_priority_fee_per_gas: max_priority_fee.unwrap_or_else(U256::zero),
					gas_limit: self.gas_limit,
					action: self.action,
					value: self.value,
					input: self.input.clone(),
					access_list: if let Some(ref access_list) = self.access_list {
						from_tuple_to_access_list(access_list)
					} else {
						Vec::new()
					},
					odd_y_parity: true,
					r: rs_id(),
					s: rs_id(),
				}))
			}
			_ => return None,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn test_into_ethereum_tx_with_low_fee() {
		let xcm_transaction = EthereumXcmTransactionV1 {
			gas_limit: U256::from(1),
			fee_payment: EthereumXcmFee::Auto(AutoEthereumXcmFee::Low),
			action: TransactionAction::Create,
			value: U256::from(0),
			input: vec![1u8],
			access_list: None,
		};
		let nonce = U256::from(0);
		let base_fee = U256::from(1);
		let expected_tx = Some(TransactionV2::EIP1559(EIP1559Transaction {
			chain_id: 0,
			nonce,
			max_fee_per_gas: base_fee,
			max_priority_fee_per_gas: U256::from(0),
			gas_limit: U256::from(1),
			action: TransactionAction::Create,
			value: U256::from(0),
			input: vec![1u8],
			access_list: vec![],
			odd_y_parity: true,
			r: H256::from_low_u64_be(1u64),
			s: H256::from_low_u64_be(1u64),
		}));

		assert_eq!(
			xcm_transaction.into_transaction_v2(base_fee, nonce),
			expected_tx
		);
	}

	#[test]
	fn test_into_ethereum_tx_with_medium_fee() {
		let xcm_transaction = EthereumXcmTransactionV1 {
			gas_limit: U256::from(1),
			fee_payment: EthereumXcmFee::Auto(AutoEthereumXcmFee::Medium),
			action: TransactionAction::Create,
			value: U256::from(0),
			input: vec![1u8],
			access_list: None,
		};
		let nonce = U256::from(0);
		let base_fee = U256::from(1);
		let expected_tx = Some(TransactionV2::EIP1559(EIP1559Transaction {
			chain_id: 0,
			nonce,
			max_fee_per_gas: base_fee * 2,
			max_priority_fee_per_gas: base_fee,
			gas_limit: U256::from(1),
			action: TransactionAction::Create,
			value: U256::from(0),
			input: vec![1u8],
			access_list: vec![],
			odd_y_parity: true,
			r: H256::from_low_u64_be(1u64),
			s: H256::from_low_u64_be(1u64),
		}));

		assert_eq!(
			xcm_transaction.into_transaction_v2(base_fee, nonce),
			expected_tx
		);
	}

	#[test]
	fn test_into_ethereum_tx_with_high_fee() {
		let xcm_transaction = EthereumXcmTransactionV1 {
			gas_limit: U256::from(1),
			fee_payment: EthereumXcmFee::Auto(AutoEthereumXcmFee::High),
			action: TransactionAction::Create,
			value: U256::from(0),
			input: vec![1u8],
			access_list: None,
		};
		let nonce = U256::from(0);
		let base_fee = U256::from(1);
		let expected_tx = Some(TransactionV2::EIP1559(EIP1559Transaction {
			chain_id: 0,
			nonce,
			max_fee_per_gas: base_fee * 3,
			max_priority_fee_per_gas: base_fee * 2,
			gas_limit: U256::from(1),
			action: TransactionAction::Create,
			value: U256::from(0),
			input: vec![1u8],
			access_list: vec![],
			odd_y_parity: true,
			r: H256::from_low_u64_be(1u64),
			s: H256::from_low_u64_be(1u64),
		}));

		assert_eq!(
			xcm_transaction.into_transaction_v2(base_fee, nonce),
			expected_tx
		);
	}
}
