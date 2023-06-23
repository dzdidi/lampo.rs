//! Handler module implementation that
use std::cell::RefCell;
use std::sync::Arc;

use bitcoin::hashes::hex::ToHex;
use lightning::events::Event;

use lampo_common::error;
use lampo_common::types::ChannelState;

use crate::chain::{LampoChainManager, WalletManager};
use crate::events::LampoEvent;
use crate::handler::external_handler::ExternalHandler;
use crate::ln::events::{ChangeStateChannelEvent, ChannelEvents, PeerEvents};
use crate::ln::{LampoChannelManager, LampoInventoryManager, LampoPeerManager};
use crate::{async_run, LampoDeamon};

use super::{Handler, InventoryHandler};

pub struct LampoHandler {
    channel_manager: Arc<LampoChannelManager>,
    peer_manager: Arc<LampoPeerManager>,
    inventory_manager: Arc<LampoInventoryManager>,
    wallet_manager: Arc<dyn WalletManager>,
    chain_manager: Arc<LampoChainManager>,
    external_handlers: RefCell<Vec<Arc<dyn ExternalHandler>>>,
}

unsafe impl Send for LampoHandler {}
unsafe impl Sync for LampoHandler {}

impl LampoHandler {
    pub fn new(lampod: &LampoDeamon) -> Self {
        Self {
            channel_manager: lampod.channel_manager(),
            peer_manager: lampod.peer_manager(),
            inventory_manager: lampod.inventory_manager(),
            wallet_manager: lampod.wallet_manager(),
            chain_manager: lampod.onchain_manager(),
            external_handlers: RefCell::new(Vec::new()),
        }
    }

    pub fn add_external_handler(&self, handler: Arc<dyn ExternalHandler>) -> error::Result<()> {
        let mut vect = self.external_handlers.borrow_mut();
        vect.push(handler);
        Ok(())
    }
}

#[allow(unused_variables)]
impl Handler for LampoHandler {
    fn react(&self, event: crate::events::LampoEvent) -> error::Result<()> {
        match event {
            LampoEvent::LNEvent() => unimplemented!(),
            LampoEvent::OnChainEvent() => unimplemented!(),
            LampoEvent::PeerEvent(event) => {
                async_run!(self.peer_manager.handle(event))
            }
            LampoEvent::InventoryEvent(event) => {
                self.inventory_manager.handle(event)?;
                Ok(())
            }
            LampoEvent::ExternalEvent(req, chan) => {
                log::info!(
                    "external handler size {}",
                    self.external_handlers.borrow().len()
                );
                for handler in self.external_handlers.borrow().iter() {
                    if let Some(resp) = handler.handle(&req)? {
                        chan.send(resp)?;
                        return Ok(());
                    }
                }
                error::bail!("method `{}` not found", req.method);
            }
        }
    }

    /// method used to handle the incoming event from ldk
    fn handle(&self, event: lightning::events::Event) -> error::Result<()> {
        match event {
            Event::OpenChannelRequest {
                temporary_channel_id,
                counterparty_node_id,
                funding_satoshis,
                push_msat,
                channel_type,
            } => {
                unimplemented!()
            }
            Event::ChannelReady {
                channel_id,
                user_channel_id,
                counterparty_node_id,
                channel_type,
            } => {
                log::info!("channel ready with node `{counterparty_node_id}`, and channel type {channel_type}");
                let event = ChangeStateChannelEvent {
                    channel_id,
                    node_id: counterparty_node_id,
                    channel_type,
                    state: ChannelState::Ready,
                };
                self.channel_manager.change_state_channel(event)
            }
            Event::ChannelClosed {
                channel_id,
                user_channel_id,
                reason,
            } => {
                log::info!("channel `{user_channel_id}` closed with reason: `{reason}`");
                Ok(())
            }
            Event::FundingGenerationReady {
                temporary_channel_id,
                counterparty_node_id,
                channel_value_satoshis,
                output_script,
                ..
            } => {
                log::info!("propagate funding transaction for open a channel with `{counterparty_node_id}`");
                // FIXME: estimate the fee rate with a callback
                let fee = self.chain_manager.backend.fee_rate_estimation(6);
                log::info!("fee estimated {fee} sats");
                let transaction = self.wallet_manager.create_transaction(
                    output_script,
                    channel_value_satoshis,
                    fee,
                )?;
                log::info!("funding transaction created `{}`", transaction.txid());
                log::info!(
                    "transaction hex `{}`",
                    lampo_common::bitcoin::consensus::serialize(&transaction).to_hex()
                );
                self.channel_manager
                    .manager()
                    .funding_transaction_generated(
                        &temporary_channel_id,
                        &counterparty_node_id,
                        transaction,
                    )
                    .map_err(|err| error::anyhow!("{:?}", err))?;
                Ok(())
            }
            Event::ChannelPending {
                counterparty_node_id,
                funding_txo,
                ..
            } => {
                log::info!(
                    "channel pending with node `{}` with funding `{funding_txo}`",
                    counterparty_node_id.to_hex()
                );
                Ok(())
            }
            _ => unreachable!("{:?}", event),
        }
    }
}
