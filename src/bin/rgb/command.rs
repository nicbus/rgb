// RGB smart contracts for Bitcoin & Lightning
//
// SPDX-License-Identifier: Apache-2.0
//
// Written in 2019-2023 by
//     Dr Maxim Orlovsky <orlovsky@lnp-bp.org>
//
// Copyright (C) 2019-2023 LNP/BP Standards Association. All rights reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use amplify::confinement::U16;
use bitcoin::bip32::ExtendedPubKey;
use bitcoin::psbt::Psbt;
use bp::Outpoint;
use bp::seals::txout::{CloseMethod, ExplicitSeal, TxPtr};
use rgb::{Runtime, RuntimeError};
use rgbstd::containers::{Bindle, Transfer, UniversalBindle};
use rgbstd::contract::{ContractId, GenesisSeal, GraphSeal, StateType};
use rgbstd::interface::{ContractBuilder, SchemaIfaces, TypedState, rgb20};
use rgbstd::persistence::{Inventory, Stash};
use rgbstd::schema::SchemaId;
use rgbstd::Txid;
use rgbstd::stl::{DivisibleAssetSpec, Precision, RicardianContract, Timestamp};
use rgbwallet::psbt::opret::OutputOpret;
use rgbwallet::psbt::tapret::OutputTapret;
use rgbwallet::{InventoryWallet, RgbInvoice, RgbTransport};
use strict_types::encoding::{FieldName, Ident, TypeName};
use strict_types::StrictVal;
use rgb_schemata::{nia_rgb20, nia_schema};

// TODO: For now, serde implementation doesn't work for consignments due to
//       some of the keys which can't be serialized to strings. Once this fixed,
//       allow this inspect formats option
/*
#[derive(ValueEnum, Copy, Clone, Eq, PartialEq, Hash, Debug, Display, Default)]
#[display(lowercase)]
pub enum InspectFormat {
    #[default]
    Yaml,
    Toml,
    Json,
    Debug,
    Contractum,
}
 */

#[derive(Subcommand, Clone, PartialEq, Eq, Debug, Display)]
#[display(lowercase)]
pub enum Command {
    /// Prints out list of known RGB schemata.
    Schemata,
    /// Prints out list of known RGB interfaces.
    Interfaces,
    /// Prints out list of known RGB contracts.
    Contracts,
    /// Prints out list of wallets.
    Wallets {
        /// Print out full descriptor with all tapret commitments.
        #[clap(short, long)]
        long: bool,
    },

    /// Create a new wallet (only key-spent only taproot wallets are supported).
    Create {
        /// Name of the new wallet
        name: Ident,

        /// Extended public key (account-level) to create a new wallet using
        /// key-only taproot descriptor.
        xpub: ExtendedPubKey,
    },

    /// Display list of UTXOs for a given wallet.
    Utxos {
        /// Wallet to filter the state.
        #[clap(short, long, default_value = "default")]
        wallet: Ident,
    },

    /// Imports RGB data into the stash: contracts, schema, interfaces, etc.
    #[display("import")]
    Import {
        /// Use BASE64 ASCII armoring for binary data.
        #[clap(short)]
        armored: bool,

        /// File with RGB data. If not provided, assumes `-a` and prints out
        /// data to STDOUT.
        file: PathBuf,
    },

    /// Exports existing RGB contract.
    #[display("export")]
    Export {
        /// Use BASE64 ASCII armoring for binary data.
        #[clap(short)]
        armored: bool,

        /// Contract to export.
        contract: ContractId,

        /// File with RGB data. If not provided, assumes `-a` and reads the data
        /// from STDIN.
        file: Option<PathBuf>,
    },

    /// Reports information about state of a contract.
    #[display("state")]
    State {
        /// Wallet to filter the state.
        #[clap(short, long)]
        wallet: Option<Ident>,

        /// Contract identifier.
        contract_id: ContractId,
        /// Interface to interpret the state data.
        iface: String,
    },

    /// Issues new contract.
    #[display("issue")]
    Issue {
        /// UTXO where to issue the asset
        utxo: String,
    },

    /// Create new invoice.
    #[display("invoice")]
    Invoice {
        /// Contract identifier.
        contract_id: ContractId,

        /// Interface to interpret the state data.
        iface: String,

        /// Value to transfer.
        value: u64,

        /// Seal to get the transfer to.
        seal: ExplicitSeal<TxPtr>,
    },

    /// Create new transfer.
    #[display("transfer")]
    Transfer {
        #[clap(long, default_value = "tapret1st")]
        /// Method for single-use-seals.
        method: CloseMethod,

        /// PSBT file.
        psbt_file: PathBuf,

        /// Invoice data.
        invoice: RgbInvoice,

        /// Filename to save transfer consignment.
        out_file: PathBuf,
    },

    /// Inspects any RGB data file.
    #[display("inspect")]
    Inspect {
        // #[clap(short, long, default_value = "yaml")]
        // /// Format used for data inspection
        // format: InspectFormat,
        /// RGB file to inspect.
        file: PathBuf,
    },

    /// Debug-dump all stash and inventory data.
    #[display("dump")]
    Dump {
        /// Directory to put the dump into.
        #[clap(default_value = "./rgb-dump")]
        root_dir: String,
    },

    /// Validate transfer consignment.
    #[display("validate")]
    Validate {
        /// File with the transfer consignment.
        file: PathBuf,
    },

    /// Validate transfer consignment & accept to the stash.
    #[display("accept")]
    Accept {
        /// Force accepting consignments with non-mined terminal witness.
        #[clap(short, long)]
        force: bool,

        /// File with the transfer consignment.
        file: PathBuf,
    },

    /// Set first opret/tapret output to host a commitment
    #[display("set-host")]
    SetHost {
        #[clap(long, default_value = "tapret1st")]
        /// Method for single-use-seals.
        method: CloseMethod,

        /// PSBT file.
        psbt_file: PathBuf,
    },
}

impl Command {
    pub fn exec(self, runtime: &mut Runtime) -> Result<(), RuntimeError> {
        match self {
            Command::Schemata => {
                for id in runtime.schema_ids()? {
                    print!("{id} ");
                    for iimpl in runtime.schema(id)?.iimpls.values() {
                        let iface = runtime.iface_by_id(iimpl.iface_id)?;
                        print!("{} ", iface.name);
                    }
                    println!();
                }
            }
            Command::Interfaces => {
                for (id, name) in runtime.ifaces()? {
                    println!("{} {id}", name);
                }
            }
            Command::Contracts => {
                for id in runtime.contract_ids()? {
                    println!("{id}");
                }
            }

            Command::Wallets { long: details } => {
                for (name, descriptor) in runtime.wallets() {
                    if details {
                        println!("{name} {descriptor:#}");
                    } else {
                        println!("{name} {descriptor}");
                    }
                }
            }

            Command::Create { name, xpub } => {
                let descr = runtime.create_wallet(&name, xpub)?;
                println!("Created new wallet '{name}' with descriptor '{descr}'");
            }

            Command::Utxos { wallet } => {
                let wallet = runtime.wallet(&wallet)?;
                for utxo in &wallet.utxos {
                    println!(
                        "outpoint={}, height={}, amount={}, derivation={}",
                        utxo.outpoint, utxo.status, utxo.amount, utxo.derivation
                    );
                }
            }

            Command::Import { armored, file } => {
                if armored {
                    todo!()
                } else {
                    let bindle = UniversalBindle::load(file)?;
                    match bindle {
                        UniversalBindle::Iface(iface) => {
                            let id = iface.id();
                            let name = iface.name.clone();
                            runtime.import_iface(iface)?;
                            eprintln!("Interface {id} with name {name} imported to the stash");
                        }
                        UniversalBindle::Schema(schema) => {
                            let id = schema.id();
                            runtime.import_schema(schema)?;
                            eprintln!("Schema {id} imported to the stash");
                        }
                        UniversalBindle::Impl(iimpl) => {
                            let iface_id = iimpl.iface_id;
                            let schema_id = iimpl.schema_id;
                            let id = iimpl.id();
                            runtime.import_iface_impl(iimpl)?;
                            eprintln!(
                                "Implementation {id} of interface {iface_id} for schema \
                                 {schema_id} imported to the stash"
                            );
                        }
                        UniversalBindle::Contract(bindle) => {
                            let id = bindle.id();
                            let contract =
                                bindle
                                    .unbindle()
                                    .validate(runtime.resolver())
                                    .map_err(|c| {
                                        c.validation_status().expect("just validated").to_string()
                                    })?;
                            runtime.import_contract(contract)?;
                            eprintln!("Contract {id} imported to the stash");
                        }
                        UniversalBindle::Transfer(_) => {
                            return Err(s!("use `validate` and `accept` commands to work with \
                                           transfer consignments")
                            .into());
                        }
                    };
                }
            }
            Command::Export {
                armored: _,
                contract,
                file,
            } => {
                let bindle = runtime
                    .export_contract(contract)
                    .map_err(|err| err.to_string())?;
                if let Some(file) = file {
                    // TODO: handle armored flag
                    bindle.save(&file)?;
                    eprintln!("Contract {contract} exported to '{}'", file.display());
                } else {
                    println!("{bindle}");
                }
            }

            Command::State {
                wallet,
                contract_id,
                iface,
            } => {
                let wallet = wallet.map(|w| runtime.wallet(&w)).transpose()?;

                let iface = runtime.iface_by_name(&tn!(iface))?.clone();
                let contract = runtime.contract_iface(contract_id, iface.iface_id())?;

                println!("Global:");
                for global in &contract.iface.global_state {
                    if let Ok(values) = contract.global(global.name.clone()) {
                        for val in values {
                            println!("  {} := {}", global.name, val);
                        }
                    }
                }

                println!("\nOwned:");
                for owned in &contract.iface.assignments {
                    println!("  {}:", owned.name);
                    let none: Option<&Vec<Outpoint>> = None;
                    if let Ok(allocations) = contract.fungible(owned.name.clone(), none) {
                        for allocation in allocations {
                            if let Some(utxo) =
                                wallet.as_ref().and_then(|w| w.utxo(allocation.owner))
                            {
                                println!(
                                    "    amount={}, utxo={}, witness={}, derivation={}",
                                    allocation.value,
                                    allocation.owner,
                                    allocation.witness,
                                    utxo.derivation
                                );
                            } else {
                                println!(
                                    "    amount={}, utxo={}, witness={} # owner unknown",
                                    allocation.value, allocation.owner, allocation.witness
                                );
                            }
                        }
                    }
                    // TODO: Print out other types of state
                }
            }
            Command::Issue {
                utxo,
            } => {
                let mut builder = ContractBuilder::with(rgb20(), nia_schema(), nia_rgb20()).unwrap()
                    .set_chain(runtime.chain());

                let spec = DivisibleAssetSpec::new("USDT", "USD Tether", Precision::Indivisible);

                let terms = RicardianContract::default();
                println!("TERMS: {terms:?}");

                let created = Timestamp::default();

                builder = builder
                    .add_global_state("spec", spec)
                    .expect("invalid spec")
                    .add_global_state("terms", terms)
                    .expect("invalid terms")
                    .add_global_state("created", created)
                    .expect("invalid created");

                let seal =
                    ExplicitSeal::<Txid>::from_str(&format!("opret1st:{utxo}")).unwrap();
                let seal = GenesisSeal::from(seal);
                builder = builder
                    .add_fungible_state("assetOwner", seal, 2000)
                    .expect("invalid global state data");

                let contract = builder.issue_contract().expect("failure issuing contract");
                let validated_contract = contract
                    .validate(runtime.resolver())
                    .map_err(|_| RuntimeError::IncompleteContract)?;
                runtime
                    .import_contract(validated_contract)
                    .expect("failure importing issued contract");
            }
            Command::Invoice {
                contract_id,
                iface,
                value,
                seal,
            } => {
                let iface = TypeName::try_from(iface).expect("invalid interface name");
                let seal = GraphSeal::from(seal);
                let invoice = RgbInvoice {
                    transports: vec![RgbTransport::UnspecifiedMeans],
                    contract: Some(contract_id),
                    iface: Some(iface),
                    operation: None,
                    assignment: None,
                    beneficiary: seal.to_concealed_seal().into(),
                    owned_state: TypedState::Amount(value),
                    chain: None,
                    expiry: None,
                    unknown_query: none!(),
                };
                runtime.store_seal_secret(seal)?;
                println!("{invoice}");
            }
            Command::Transfer {
                method,
                psbt_file,
                invoice,
                out_file,
            } => {
                // TODO: Check PSBT format
                let psbt_data = fs::read(&psbt_file)?;
                let mut psbt = Psbt::deserialize(&psbt_data)?;
                let transfer = runtime
                    .pay(invoice, &mut psbt, method)
                    .map_err(|err| err.to_string())?;
                fs::write(&psbt_file, psbt.serialize())?;
                // TODO: Print PSBT as Base64
                transfer.save(&out_file)?;
                eprintln!("Transfer is created and saved into '{}'.", out_file.display());
                eprintln!(
                    "PSBT file '{}' is updated with all required commitments and ready to be \
                     signed.",
                    psbt_file.display()
                );
                eprintln!("Stash data are updated.");
            }
            Command::Inspect { file } => {
                let bindle = UniversalBindle::load(file)?;
                // TODO: For now, serde implementation doesn't work for consignments due to
                //       some of the keys which can't be serialized to strings. Once this fixed,
                //       allow this inspect formats option
                /* let s = match format {
                    InspectFormat::Yaml => {
                        serde_yaml::to_string(&bindle).expect("unable to present as YAML")
                    }
                    InspectFormat::Toml => {
                        toml::to_string(&bindle).expect("unable to present as TOML")
                    }
                    InspectFormat::Json => {
                        serde_json::to_string(&bindle).expect("unable to present as JSON")
                    }
                    InspectFormat::Debug => format!("{bindle:#?}"),
                    InspectFormat::Contractum => todo!("contractum representation"),
                };
                println!("{s}");
                 */
                println!("{bindle:#?}");
            }
            Command::Dump { root_dir } => {
                fs::remove_dir_all(&root_dir).ok();
                fs::create_dir_all(format!("{root_dir}/stash/schemata"))?;
                fs::create_dir_all(format!("{root_dir}/stash/ifaces"))?;
                fs::create_dir_all(format!("{root_dir}/stash/geneses"))?;
                fs::create_dir_all(format!("{root_dir}/stash/bundles"))?;
                fs::create_dir_all(format!("{root_dir}/stash/anchors"))?;
                fs::create_dir_all(format!("{root_dir}/stash/extensions"))?;
                fs::create_dir_all(format!("{root_dir}/state"))?;
                fs::create_dir_all(format!("{root_dir}/index"))?;

                // Stash
                for id in runtime.schema_ids()? {
                    fs::write(
                        format!("{root_dir}/stash/schemata/{id}.debug"),
                        format!("{:#?}", runtime.schema(id)?),
                    )?;
                }
                for (id, name) in runtime.ifaces()? {
                    fs::write(
                        format!("{root_dir}/stash/ifaces/{id}.{name}.debug"),
                        format!("{:#?}", runtime.iface_by_id(id)?),
                    )?;
                }
                for id in runtime.contract_ids()? {
                    fs::write(
                        format!("{root_dir}/stash/geneses/{id}.debug"),
                        format!("{:#?}", runtime.genesis(id)?),
                    )?;
                    for (no, suppl) in runtime.contract_suppl(id).into_iter().flatten().enumerate()
                    {
                        fs::write(
                            format!("{root_dir}/stash/geneses/{id}.suppl.{no:03}.debug"),
                            format!("{:#?}", suppl),
                        )?;
                    }
                }
                for id in runtime.bundle_ids()? {
                    fs::write(
                        format!("{root_dir}/stash/bundles/{id}.debug"),
                        format!("{:#?}", runtime.bundle(id)?),
                    )?;
                }
                for id in runtime.anchor_ids()? {
                    fs::write(
                        format!("{root_dir}/stash/anchors/{id}.debug"),
                        format!("{:#?}", runtime.anchor(id)?),
                    )?;
                }
                for id in runtime.extension_ids()? {
                    fs::write(
                        format!("{root_dir}/stash/extensions/{id}.debug"),
                        format!("{:#?}", runtime.extension(id)?),
                    )?;
                }
                // TODO: Add sigs debugging

                // State
                for (id, history) in runtime.debug_history() {
                    fs::write(format!("{root_dir}/state/{id}.debug"), format!("{:#?}", history))?;
                }

                // Index
                fs::write(
                    format!("{root_dir}/index/op-to-bundle.debug"),
                    format!("{:#?}", runtime.debug_bundle_op_index()),
                )?;
                fs::write(
                    format!("{root_dir}/index/bundle-to-anchor.debug"),
                    format!("{:#?}", runtime.debug_anchor_bundle_index()),
                )?;
                fs::write(
                    format!("{root_dir}/index/contracts.debug"),
                    format!("{:#?}", runtime.debug_contract_index()),
                )?;
                fs::write(
                    format!("{root_dir}/index/terminals.debug"),
                    format!("{:#?}", runtime.debug_terminal_index()),
                )?;
                fs::write(
                    format!("{root_dir}/seal-secret.debug"),
                    format!("{:#?}", runtime.debug_seal_secrets()),
                )?;
                eprintln!("Dump is successfully generated and saved to '{root_dir}'");
            }
            Command::Validate { file } => {
                let bindle = Bindle::<Transfer>::load(file)?;
                let status = match bindle.unbindle().validate(runtime.resolver()) {
                    Ok(consignment) => consignment.into_validation_status(),
                    Err(consignment) => consignment.into_validation_status(),
                }
                .expect("just validated");
                eprintln!("{status}");
            }
            Command::Accept { force, file } => {
                let bindle = Bindle::<Transfer>::load(file)?;
                let transfer = bindle
                    .unbindle()
                    .validate(runtime.resolver())
                    .unwrap_or_else(|c| c);
                eprintln!("{}", transfer.validation_status().expect("just validated"));
                runtime.accept_transfer(transfer, force)?;
                eprintln!("Transfer accepted into the stash");
            }
            Command::SetHost { method, psbt_file } => {
                let psbt_data = fs::read(&psbt_file)?;
                let mut psbt = Psbt::deserialize(&psbt_data)?;
                let mut psbt_modified = false;
                match method {
                    CloseMethod::OpretFirst => {
                        psbt.unsigned_tx
                            .output
                            .iter()
                            .zip(&mut psbt.outputs)
                            .find(|(o, outp)| {
                                o.script_pubkey.is_op_return() && !outp.is_opret_host()
                            })
                            .and_then(|(_, outp)| {
                                psbt_modified = true;
                                outp.set_opret_host().ok()
                            });
                    }
                    CloseMethod::TapretFirst => {
                        psbt.unsigned_tx
                            .output
                            .iter()
                            .zip(&mut psbt.outputs)
                            .find(|(o, outp)| {
                                o.script_pubkey.is_v1_p2tr() && !outp.is_tapret_host()
                            })
                            .and_then(|(_, outp)| {
                                psbt_modified = true;
                                outp.set_tapret_host().ok()
                            });
                    }
                    _ => {}
                };
                fs::write(&psbt_file, psbt.serialize())?;
                if psbt_modified {
                    eprintln!(
                        "PSBT file '{}' is updated with {method} host now set.",
                        psbt_file.display()
                    );
                }
            }
        }

        Ok(())
    }
}
