use crate::log_entry::LogEntry;
use crate::prelude::account_id::AccountId;
use crate::prelude::{
    validate_eth_address, vec, AddressValidationError, Balance, BorshDeserialize, BorshSerialize,
    EthAddress, Fee, String, ToString, TryFrom, Vec,
};

use crate::deposit_event::error::ParseEventMessageError;
use ethabi::{Event, EventParam, Hash, Log, ParamType, RawLog};

pub const DEPOSITED_EVENT: &str = "Deposited";

pub type EventParams = Vec<EventParam>;

/// Token message data used for Deposit flow.
/// It contains two basic data structure: Near, Eth
/// The message parsed from event `recipient` field - `log_entry_data`
/// after fetching proof `log_entry_data`
#[derive(BorshSerialize, BorshDeserialize)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
pub enum TokenMessageData {
    /// Deposit no NEAR account
    Near(AccountId),
    ///Deposit to Eth accounts fee is being minted in the `ft_on_transfer` callback method
    Eth {
        receiver_id: AccountId,
        message: String,
    },
}

impl TokenMessageData {
    /// Parse event message data for tokens. Data parsed form event `recipient` field.
    /// Used for Deposit flow.
    /// For Eth logic flow message validated and prepared for  `ft_on_transfer` logic.
    /// It mean validating Eth address correctness and preparing message for
    /// parsing for `ft_on_transfer` message parsing with correct and validated data.
    pub fn parse_event_message_and_prepare_token_message_data(
        message: &str,
        fee: Fee,
    ) -> Result<TokenMessageData, error::ParseEventMessageError> {
        let data: Vec<_> = message.split(':').collect();
        // Data array can contain 1 or 2 elements
        if data.len() >= 3 {
            return Err(error::ParseEventMessageError::TooManyParts);
        }
        let account_id = AccountId::try_from(data[0].as_bytes())
            .map_err(|_| error::ParseEventMessageError::InvalidAccount)?;

        // If data array contain only one element it should return NEAR account id
        if data.len() == 1 {
            Ok(TokenMessageData::Near(account_id))
        } else {
            let raw_message = data[1].into();
            let message = Self::prepare_message_for_on_transfer(&account_id, fee, raw_message)?;

            Ok(TokenMessageData::Eth {
                receiver_id: account_id,
                message,
            })
        }
    }

    // Get recipient account id from Eth part of Token message data
    pub fn get_recipient(&self) -> AccountId {
        match self {
            Self::Near(acc) => acc.clone(),
            Self::Eth {
                receiver_id,
                message: _,
            } => receiver_id.clone(),
        }
    }

    /// Prepare message for `ft_transfer_call` -> `ft_on_transfer`
    fn prepare_message_for_on_transfer(
        relayer_account_id: &AccountId,
        fee: Fee,
        message: String,
    ) -> Result<String, ParseEventMessageError> {
        // First data section should contain fee data
        let mut data = fee.into_u128().to_be_bytes().to_vec();

        // Check message length.Ω
        let address = if message.len() == 42 {
            message
                .strip_prefix("0x")
                .ok_or(ParseEventMessageError::EthAddressValidationError(
                    AddressValidationError::FailedDecodeHex,
                ))?
                .to_string()
        } else {
            message
        };
        let address_bytes = validate_eth_address(address)
            .map_err(ParseEventMessageError::EthAddressValidationError)?;
        // Second data section should contain Eth address
        data.extend(address_bytes);
        // Add `:` separator between relayer_id and data message
        Ok([relayer_account_id.as_ref(), &hex::encode(data)].join(":"))
    }
}

/// Ethereum event
pub struct EthEvent {
    pub eth_custodian_address: EthAddress,
    pub log: Log,
}

#[allow(dead_code)]
impl EthEvent {
    /// Get Ethereum event from `log_entry_data`
    pub fn fetch_log_entry_data(
        name: &str,
        params: EventParams,
        data: &[u8],
    ) -> Result<Self, error::DecodeError> {
        let event = Event {
            name: name.to_string(),
            inputs: params,
            anonymous: false,
        };
        let log_entry: LogEntry = rlp::decode(data).map_err(|_| error::DecodeError::RlpFailed)?;
        let eth_custodian_address = log_entry.address.0;
        let topics = log_entry.topics.iter().map(|h| Hash::from(h.0)).collect();

        let raw_log = RawLog {
            topics,
            data: log_entry.data,
        };
        let log = event
            .parse_log(raw_log)
            .map_err(|_| error::DecodeError::SchemaMismatch)?;

        Ok(Self {
            eth_custodian_address,
            log,
        })
    }
}

/// Data that was emitted by Deposited event.
pub struct DepositedEvent {
    pub eth_custodian_address: EthAddress,
    pub sender: EthAddress,
    pub token_message_data: TokenMessageData,
    pub amount: Balance,
    pub fee: Fee,
}

impl DepositedEvent {
    #[allow(dead_code)]
    pub fn event_params() -> EventParams {
        vec![
            EventParam {
                name: "sender".to_string(),
                kind: ParamType::Address,
                indexed: true,
            },
            EventParam {
                name: "recipient".to_string(),
                kind: ParamType::String,
                indexed: false,
            },
            EventParam {
                name: "amount".to_string(),
                kind: ParamType::Uint(256),
                indexed: false,
            },
            EventParam {
                name: "fee".to_string(),
                kind: ParamType::Uint(256),
                indexed: false,
            },
        ]
    }

    /// Parses raw Ethereum logs proof's entry data
    pub fn from_log_entry_data(data: &[u8]) -> Result<Self, error::ParseError> {
        let event = EthEvent::fetch_log_entry_data(DEPOSITED_EVENT, Self::event_params(), data)
            .map_err(error::ParseError::LogParseFailed)?;
        let sender = event.log.params[0]
            .value
            .clone()
            .into_address()
            .ok_or(error::ParseError::InvalidSender)?
            .0;

        // parse_event_message
        let event_message_data: String = event.log.params[1].value.clone().to_string();

        let amount = event.log.params[2]
            .value
            .clone()
            .into_uint()
            .ok_or(error::ParseError::InvalidAmount)?
            .as_u128();
        let fee: Fee = event.log.params[3]
            .value
            .clone()
            .into_uint()
            .ok_or(error::ParseError::InvalidFee)?
            .as_u128()
            .into();

        let token_message_data =
            TokenMessageData::parse_event_message_and_prepare_token_message_data(
                &event_message_data,
                fee,
            )?;

        Ok(Self {
            eth_custodian_address: event.eth_custodian_address,
            sender,
            token_message_data,
            amount,
            fee,
        })
    }
}

pub mod error {
    use super::*;

    pub enum DecodeError {
        RlpFailed,
        SchemaMismatch,
    }
    impl AsRef<[u8]> for DecodeError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::RlpFailed => b"ERR_RLP_FAILED",
                Self::SchemaMismatch => b"ERR_PARSE_DEPOSIT_EVENT",
            }
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
    pub enum ParseEventMessageError {
        TooManyParts,
        InvalidAccount,
        EthAddressValidationError(AddressValidationError),
    }

    impl AsRef<[u8]> for ParseEventMessageError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::TooManyParts => b"ERR_INVALID_EVENT_MESSAGE_FORMAT",
                Self::InvalidAccount => b"ERR_INVALID_ACCOUNT_ID",
                Self::EthAddressValidationError(e) => e.as_ref(),
            }
        }
    }

    impl From<ParseEventMessageError> for ParseError {
        fn from(e: ParseEventMessageError) -> Self {
            Self::MessageParseFailed(e)
        }
    }

    pub enum ParseError {
        LogParseFailed(DecodeError),
        InvalidSender,
        InvalidAmount,
        InvalidFee,
        MessageParseFailed(ParseEventMessageError),
    }
    impl AsRef<[u8]> for ParseError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::LogParseFailed(e) => e.as_ref(),
                Self::InvalidSender => b"ERR_INVALID_SENDER",
                Self::InvalidAmount => b"ERR_INVALID_AMOUNT",
                Self::InvalidFee => b"ERR_INVALID_FEE",
                Self::MessageParseFailed(e) => e.as_ref(),
            }
        }
    }
}
