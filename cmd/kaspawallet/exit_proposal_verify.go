package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/kaspanet/kaspad/app/appmessage"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/daemon/server"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/keys"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet/bip32"
	walletserialization "github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet/serialization"
	"github.com/kaspanet/kaspad/domain/consensus/model/externalapi"
	"github.com/kaspanet/kaspad/domain/consensus/utils/consensushashing"
	"github.com/kaspanet/kaspad/domain/consensus/utils/txscript"
	"github.com/kaspanet/kaspad/domain/consensus/utils/utxo"
	"github.com/kaspanet/kaspad/domain/dagconfig"
	"github.com/kaspanet/kaspad/infrastructure/network/rpcclient"
	"github.com/kaspanet/kaspad/util"
	"github.com/pkg/errors"
)

const exitProposalEvidenceKind = "kaspa-exit-proposal-evidence"

type exitProposalVerifyOptions struct {
	KeysFile     string
	ProposalFile string
	EvidenceFile string
	SafeURL      string
	ProposalHash string
	IgraRPCURL   string
	KaspaRPCURL  string
	JSON         bool
	NetParams    *dagconfig.Params
}

type exitProposalMaterial struct {
	Proposal               exitProposalAPI
	Evidence               exitProposalEvidence
	EvidenceHash           string
	UnsignedBundleParts    [][]byte
	PartiallySignedTx      *walletserialization.PartiallySignedTransaction
	PartiallySignedTxBytes []byte
	Result                 exitProposalVerificationResult
}

type exitProposalVerificationResult struct {
	OK           bool     `json:"ok"`
	ProposalHash string   `json:"proposal_hash"`
	EvidenceHash string   `json:"evidence_hash"`
	KaspaTxID    string   `json:"kaspa_tx_id"`
	Checks       []string `json:"checks"`
	Warnings     []string `json:"warnings,omitempty"`
}

type exitProposalAPI struct {
	ID                  string          `json:"id"`
	Federation          string          `json:"federation"`
	ExitBatch           string          `json:"exit_batch"`
	ExitEvidenceHash    string          `json:"exit_evidence_hash"`
	ProposalHash        string          `json:"proposal_hash"`
	Format              string          `json:"format"`
	UnsignedBundleHex   string          `json:"unsigned_bundle_hex"`
	MergedBundleHex     string          `json:"merged_bundle_hex"`
	Status              string          `json:"status"`
	TxIDs               []string        `json:"tx_ids"`
	InputOutpoints      json.RawMessage `json:"input_outpoints"`
	Outputs             json.RawMessage `json:"outputs"`
	FeeSompi            *uint64         `json:"fee_sompi"`
	SignaturesRequired  uint32          `json:"signatures_required"`
	SignaturesCollected uint32          `json:"signatures_collected"`
	Origin              json.RawMessage `json:"origin"`
	BroadcastTxIDs      []string        `json:"broadcast_tx_ids"`
	BroadcastError      *string         `json:"broadcast_error"`
}

func (proposal *exitProposalAPI) UnmarshalJSON(data []byte) error {
	type alias exitProposalAPI
	aux := struct {
		*alias
		ExitBatchCamel           string          `json:"exitBatch"`
		ExitEvidenceHashCamel    string          `json:"exitEvidenceHash"`
		ProposalHashCamel        string          `json:"proposalHash"`
		UnsignedBundleHexCamel   string          `json:"unsignedBundleHex"`
		MergedBundleHexCamel     string          `json:"mergedBundleHex"`
		TxIDsCamel               []string        `json:"txIds"`
		InputOutpointsCamel      json.RawMessage `json:"inputOutpoints"`
		FeeSompiCamel            *uint64         `json:"feeSompi"`
		SignaturesRequiredCamel  uint32          `json:"signaturesRequired"`
		SignaturesCollectedCamel uint32          `json:"signaturesCollected"`
		BroadcastTxIDsCamel      []string        `json:"broadcastTxIds"`
		BroadcastErrorCamel      *string         `json:"broadcastError"`
	}{alias: (*alias)(proposal)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if proposal.ExitBatch == "" {
		proposal.ExitBatch = aux.ExitBatchCamel
	}
	if proposal.ExitEvidenceHash == "" {
		proposal.ExitEvidenceHash = aux.ExitEvidenceHashCamel
	}
	if proposal.ProposalHash == "" {
		proposal.ProposalHash = aux.ProposalHashCamel
	}
	if proposal.UnsignedBundleHex == "" {
		proposal.UnsignedBundleHex = aux.UnsignedBundleHexCamel
	}
	if proposal.MergedBundleHex == "" {
		proposal.MergedBundleHex = aux.MergedBundleHexCamel
	}
	if len(proposal.TxIDs) == 0 {
		proposal.TxIDs = aux.TxIDsCamel
	}
	if len(proposal.InputOutpoints) == 0 {
		proposal.InputOutpoints = aux.InputOutpointsCamel
	}
	if proposal.FeeSompi == nil {
		proposal.FeeSompi = aux.FeeSompiCamel
	}
	if proposal.SignaturesRequired == 0 {
		proposal.SignaturesRequired = aux.SignaturesRequiredCamel
	}
	if proposal.SignaturesCollected == 0 {
		proposal.SignaturesCollected = aux.SignaturesCollectedCamel
	}
	if len(proposal.BroadcastTxIDs) == 0 {
		proposal.BroadcastTxIDs = aux.BroadcastTxIDsCamel
	}
	if proposal.BroadcastError == nil {
		proposal.BroadcastError = aux.BroadcastErrorCamel
	}
	return nil
}

type exitProposalOrigin struct {
	Kind         string                `json:"kind"`
	EvidenceHash string                `json:"evidenceHash"`
	Candidate    exitProposalCandidate `json:"candidate"`
}

type exitProposalCandidate struct {
	ArtifactHashes         map[string]string      `json:"artifactHashes"`
	BuildInput             map[string]interface{} `json:"buildInput"`
	UnsignedManifest       unsignedExitManifest   `json:"unsignedManifest"`
	UnsignedVerify         unsignedVerifyReport   `json:"unsignedVerify"`
	WalletHexNormalization map[string]interface{} `json:"walletHexNormalization"`
}

type exitBatchAPI struct {
	ID           string          `json:"id"`
	EvidenceHash string          `json:"evidence_hash"`
	Evidence     json.RawMessage `json:"evidence"`
}

func (batch *exitBatchAPI) UnmarshalJSON(data []byte) error {
	type alias exitBatchAPI
	aux := struct {
		*alias
		EvidenceHashCamel string          `json:"evidenceHash"`
		EvidenceCamel     json.RawMessage `json:"evidence"`
	}{alias: (*alias)(batch)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if batch.EvidenceHash == "" {
		batch.EvidenceHash = aux.EvidenceHashCamel
	}
	if len(batch.Evidence) == 0 {
		batch.Evidence = aux.EvidenceCamel
	}
	return nil
}

type exitProposalEvidence struct {
	SchemaVersion int    `json:"schemaVersion"`
	Kind          string `json:"kind"`
	Network       struct {
		Kaspa       string `json:"kaspa"`
		IgraChainID uint64 `json:"igraChainId"`
		IgraRPCURL  string `json:"igraRpcUrl"`
	} `json:"network"`
	Window struct {
		FromBlock            uint64 `json:"fromBlock"`
		ToBlock              uint64 `json:"toBlock"`
		StartCheckpointBlock uint64 `json:"startCheckpointBlock"`
		FinalizedAtBlock     uint64 `json:"finalizedAtBlock"`
		DeltaBlocks          uint64 `json:"deltaBlocks"`
		L2ConfirmationBlocks uint64 `json:"l2ConfirmationBlocks"`
	} `json:"window"`
	Contracts struct {
		KasExitBridge  string `json:"kasExitBridge"`
		Mailbox        string `json:"mailbox"`
		MerkleTreeHook string `json:"merkleTreeHook"`
	} `json:"contracts"`
	Bridge struct {
		Address         string   `json:"address"`
		ScriptPublicKey string   `json:"scriptPublicKey"`
		DerivationPath  string   `json:"derivationPath"`
		Threshold       uint32   `json:"threshold"`
		ECDSA           bool     `json:"ecdsa"`
		XpubFingerprint string   `json:"xpubFingerprint"`
		Xpubs           []string `json:"xpubs"`
	} `json:"bridge"`
	Exits  []evidenceExit `json:"exits"`
	Bundle struct {
		Manifest          map[string]interface{} `json:"manifest"`
		ExitData          map[string]interface{} `json:"exitData"`
		Checks            map[string]interface{} `json:"checks"`
		TreeData          map[string]interface{} `json:"treeData"`
		CheckpointEnd     map[string]interface{} `json:"checkpointEnd"`
		ContractPreverify map[string]interface{} `json:"contractPreverify"`
		RawJSONArtifacts  map[string]interface{} `json:"rawJsonArtifacts"`
	} `json:"bundle"`
	KaspaTransaction struct {
		BuildInput             map[string]interface{} `json:"buildInput"`
		UnsignedManifest       unsignedExitManifest   `json:"unsignedManifest"`
		UnsignedVerify         unsignedVerifyReport   `json:"unsignedVerify"`
		WalletHexNormalization map[string]interface{} `json:"walletHexNormalization"`
	} `json:"kaspaTransaction"`
}

type evidenceExit struct {
	RequestID       uint64                 `json:"requestId"`
	MessageID       string                 `json:"messageId"`
	BlockNumber     uint64                 `json:"blockNumber"`
	TransactionHash string                 `json:"transactionHash"`
	TreeIndex       *uint64                `json:"treeIndex"`
	Recipient       string                 `json:"recipient"`
	AmountSompi     uint64                 `json:"amountSompi"`
	BurnWei         string                 `json:"burnWei"`
	OriginBurner    string                 `json:"originBurner"`
	Checks          map[string]interface{} `json:"checks"`
}

type unsignedExitManifest struct {
	Schema           string           `json:"schema"`
	Network          string           `json:"network"`
	Protocol         manifestProtocol `json:"protocol"`
	LockingUTXOs     []manifestUTXO   `json:"locking_utxos"`
	Exits            []manifestExit   `json:"exits"`
	Change           *manifestChange  `json:"change"`
	FeeSompi         uint64           `json:"fee_sompi"`
	TotalInputSompi  uint64           `json:"total_input_sompi"`
	TotalOutputSompi uint64           `json:"total_output_sompi"`
	Multisig         manifestMultisig `json:"multisig"`
	Wallet           manifestWallet   `json:"wallet"`
}

func (manifest *unsignedExitManifest) UnmarshalJSON(data []byte) error {
	type alias unsignedExitManifest
	aux := struct {
		*alias
		LockingUTXOsCamel     []manifestUTXO `json:"lockingUtxos"`
		FeeSompiCamel         uint64         `json:"feeSompi"`
		TotalInputSompiCamel  uint64         `json:"totalInputSompi"`
		TotalOutputSompiCamel uint64         `json:"totalOutputSompi"`
	}{alias: (*alias)(manifest)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if len(manifest.LockingUTXOs) == 0 {
		manifest.LockingUTXOs = aux.LockingUTXOsCamel
	}
	if manifest.FeeSompi == 0 {
		manifest.FeeSompi = aux.FeeSompiCamel
	}
	if manifest.TotalInputSompi == 0 {
		manifest.TotalInputSompi = aux.TotalInputSompiCamel
	}
	if manifest.TotalOutputSompi == 0 {
		manifest.TotalOutputSompi = aux.TotalOutputSompiCamel
	}
	return nil
}

type manifestProtocol struct {
	Version       uint32 `json:"version"`
	TxTypeID      uint32 `json:"tx_type_id"`
	PayloadHeader string `json:"payload_header"`
	TxIDPrefix    string `json:"tx_id_prefix"`
	Nonce         string `json:"nonce"`
	KaspaTxID     string `json:"kaspa_tx_id"`
	PayloadHex    string `json:"payload_hex"`
}

func (protocol *manifestProtocol) UnmarshalJSON(data []byte) error {
	type alias manifestProtocol
	aux := struct {
		*alias
		TxTypeIDCamel      uint32 `json:"txTypeId"`
		PayloadHeaderCamel string `json:"payloadHeader"`
		TxIDPrefixCamel    string `json:"txIdPrefix"`
		KaspaTxIDCamel     string `json:"kaspaTxId"`
		PayloadHexCamel    string `json:"payloadHex"`
	}{alias: (*alias)(protocol)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if protocol.TxTypeID == 0 {
		protocol.TxTypeID = aux.TxTypeIDCamel
	}
	if protocol.PayloadHeader == "" {
		protocol.PayloadHeader = aux.PayloadHeaderCamel
	}
	if protocol.TxIDPrefix == "" {
		protocol.TxIDPrefix = aux.TxIDPrefixCamel
	}
	if protocol.KaspaTxID == "" {
		protocol.KaspaTxID = aux.KaspaTxIDCamel
	}
	if protocol.PayloadHex == "" {
		protocol.PayloadHex = aux.PayloadHexCamel
	}
	return nil
}

type manifestMultisig struct {
	MinimumSignatures  uint32   `json:"minimum_signatures"`
	ExtendedPublicKeys []string `json:"extended_public_keys"`
	ECDSA              bool     `json:"ecdsa"`
}

func (multisig *manifestMultisig) UnmarshalJSON(data []byte) error {
	type alias manifestMultisig
	aux := struct {
		*alias
		MinimumSignaturesCamel  uint32   `json:"minimumSignatures"`
		ExtendedPublicKeysCamel []string `json:"extendedPublicKeys"`
	}{alias: (*alias)(multisig)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if multisig.MinimumSignatures == 0 {
		multisig.MinimumSignatures = aux.MinimumSignaturesCamel
	}
	if len(multisig.ExtendedPublicKeys) == 0 {
		multisig.ExtendedPublicKeys = aux.ExtendedPublicKeysCamel
	}
	return nil
}

type manifestWallet struct {
	Format             string `json:"format"`
	HexSha256          string `json:"hex_sha256"`
	TransactionVersion uint16 `json:"transaction_version"`
	Inputs             int    `json:"inputs"`
	Outputs            int    `json:"outputs"`
}

func (wallet *manifestWallet) UnmarshalJSON(data []byte) error {
	type alias manifestWallet
	aux := struct {
		*alias
		HexSha256Camel          string `json:"hexSha256"`
		TransactionVersionCamel uint16 `json:"transactionVersion"`
	}{alias: (*alias)(wallet)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if wallet.HexSha256 == "" {
		wallet.HexSha256 = aux.HexSha256Camel
	}
	if wallet.TransactionVersion == 0 {
		wallet.TransactionVersion = aux.TransactionVersionCamel
	}
	return nil
}

type manifestUTXO struct {
	TransactionID   string                  `json:"transaction_id"`
	Index           uint32                  `json:"index"`
	AmountSompi     uint64                  `json:"amount_sompi"`
	Address         string                  `json:"address"`
	ScriptPublicKey manifestScriptPublicKey `json:"script_public_key"`
	DerivationPath  string                  `json:"derivation_path"`
}

func (utxo *manifestUTXO) UnmarshalJSON(data []byte) error {
	type alias manifestUTXO
	aux := struct {
		*alias
		TransactionIDCamel   string                  `json:"transactionId"`
		AmountSompiCamel     uint64                  `json:"amountSompi"`
		ScriptPublicKeyCamel manifestScriptPublicKey `json:"scriptPublicKey"`
		DerivationPathCamel  string                  `json:"derivationPath"`
	}{alias: (*alias)(utxo)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if utxo.TransactionID == "" {
		utxo.TransactionID = aux.TransactionIDCamel
	}
	if utxo.AmountSompi == 0 {
		utxo.AmountSompi = aux.AmountSompiCamel
	}
	if utxo.ScriptPublicKey.Script == "" {
		utxo.ScriptPublicKey = aux.ScriptPublicKeyCamel
	}
	if utxo.DerivationPath == "" {
		utxo.DerivationPath = aux.DerivationPathCamel
	}
	return nil
}

type manifestScriptPublicKey struct {
	Version uint16 `json:"version"`
	Script  string `json:"script"`
}

type manifestExit struct {
	MessageID   string `json:"message_id"`
	Recipient   string `json:"recipient"`
	AmountSompi uint64 `json:"amount_sompi"`
}

func (exit *manifestExit) UnmarshalJSON(data []byte) error {
	type alias manifestExit
	aux := struct {
		*alias
		MessageIDCamel   string `json:"messageId"`
		AmountSompiCamel uint64 `json:"amountSompi"`
	}{alias: (*alias)(exit)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if exit.MessageID == "" {
		exit.MessageID = aux.MessageIDCamel
	}
	if exit.AmountSompi == 0 {
		exit.AmountSompi = aux.AmountSompiCamel
	}
	return nil
}

type manifestChange struct {
	DerivationPath string `json:"derivation_path"`
	AmountSompi    uint64 `json:"amount_sompi"`
	Address        string `json:"address"`
}

func (change *manifestChange) UnmarshalJSON(data []byte) error {
	type alias manifestChange
	aux := struct {
		*alias
		DerivationPathCamel string `json:"derivationPath"`
		AmountSompiCamel    uint64 `json:"amountSompi"`
	}{alias: (*alias)(change)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if change.DerivationPath == "" {
		change.DerivationPath = aux.DerivationPathCamel
	}
	if change.AmountSompi == 0 {
		change.AmountSompi = aux.AmountSompiCamel
	}
	return nil
}

type unsignedVerifyReport struct {
	OK           bool   `json:"ok"`
	KaspaTxID    string `json:"kaspa_tx_id"`
	PayloadNonce uint64 `json:"payload_nonce"`
	Inputs       int    `json:"inputs"`
	Outputs      int    `json:"outputs"`
	SignedInputs int    `json:"signed_inputs"`
	FullySigned  bool   `json:"fully_signed"`
}

func (report *unsignedVerifyReport) UnmarshalJSON(data []byte) error {
	type alias unsignedVerifyReport
	aux := struct {
		*alias
		KaspaTxIDCamel    string `json:"kaspaTxId"`
		PayloadNonceCamel uint64 `json:"payloadNonce"`
		SignedInputsCamel int    `json:"signedInputs"`
		FullySignedCamel  bool   `json:"fullySigned"`
	}{alias: (*alias)(report)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if report.KaspaTxID == "" {
		report.KaspaTxID = aux.KaspaTxIDCamel
	}
	if report.PayloadNonce == 0 {
		report.PayloadNonce = aux.PayloadNonceCamel
	}
	if report.SignedInputs == 0 {
		report.SignedInputs = aux.SignedInputsCamel
	}
	if !report.FullySigned {
		report.FullySigned = aux.FullySignedCamel
	}
	return nil
}

type expectedPayment struct {
	Address string
	Amount  uint64
}

func verifyExitProposal(conf *verifyExitProposalConfig) error {
	material, err := verifyExitProposalWithOptions(exitProposalVerifyOptions{
		KeysFile:     conf.KeysFile,
		ProposalFile: conf.ProposalFile,
		EvidenceFile: conf.EvidenceFile,
		SafeURL:      conf.SafeURL,
		ProposalHash: conf.ProposalHash,
		IgraRPCURL:   conf.IgraRPCURL,
		KaspaRPCURL:  conf.KaspaRPCURL,
		JSON:         conf.JSON,
		NetParams:    conf.NetParams(),
	})
	if err != nil {
		return err
	}

	return printExitProposalVerification(material.Result, conf.JSON)
}

func verifyExitProposalForSigning(conf *signExitProposalConfig) (*exitProposalMaterial, error) {
	return verifyExitProposalWithOptions(exitProposalVerifyOptions{
		KeysFile:     conf.KeysFile,
		ProposalFile: conf.ProposalFile,
		EvidenceFile: conf.EvidenceFile,
		SafeURL:      conf.SafeURL,
		ProposalHash: conf.ProposalHash,
		IgraRPCURL:   conf.IgraRPCURL,
		KaspaRPCURL:  conf.KaspaRPCURL,
		NetParams:    conf.NetParams(),
	})
}

func verifyExitProposalWithOptions(opts exitProposalVerifyOptions) (*exitProposalMaterial, error) {
	if opts.NetParams == nil {
		return nil, errors.New("network parameters are required")
	}

	keysFile, err := keys.ReadKeysFile(opts.NetParams, opts.KeysFile)
	if err != nil {
		return nil, err
	}

	proposal, evidenceRaw, err := loadExitProposalAndEvidence(opts)
	if err != nil {
		return nil, err
	}
	evidenceHash, _, err := canonicalJSONHashBytes(evidenceRaw)
	if err != nil {
		return nil, errors.Wrap(err, "failed to hash exit evidence")
	}

	var evidence exitProposalEvidence
	if err := json.Unmarshal(evidenceRaw, &evidence); err != nil {
		return nil, errors.Wrap(err, "failed to decode exit evidence")
	}

	result := exitProposalVerificationResult{
		ProposalHash: proposal.ProposalHash,
		EvidenceHash: evidenceHash,
	}
	addCheck := func(check string) {
		result.Checks = append(result.Checks, check)
	}

	if proposal.Format != "kaspawallet_pst_v1" {
		return nil, errors.Errorf("unsupported proposal format %q", proposal.Format)
	}
	addCheck("proposal format is kaspawallet_pst_v1")
	if proposal.ProposalHash == "" {
		return nil, errors.New("proposal hash is empty")
	}
	if strings.TrimSpace(proposal.UnsignedBundleHex) == "" {
		return nil, errors.New("proposal unsigned bundle is empty")
	}
	if proposal.ExitEvidenceHash != "" && proposal.ExitEvidenceHash != evidenceHash {
		return nil, errors.Errorf("evidence hash mismatch: proposal has %s, evidence hashes to %s", proposal.ExitEvidenceHash, evidenceHash)
	}
	addCheck("proposal evidence hash matches evidence JSON")
	if evidence.Kind != exitProposalEvidenceKind {
		return nil, errors.Errorf("unsupported evidence kind %q", evidence.Kind)
	}
	if evidence.SchemaVersion != 1 {
		return nil, errors.Errorf("unsupported evidence schema version %d", evidence.SchemaVersion)
	}
	addCheck("evidence schema is supported")

	candidate, err := proposalCandidateMaterial(proposal, evidence)
	if err != nil {
		return nil, err
	}
	manifest := candidate.UnsignedManifest
	verifyReport := candidate.UnsignedVerify
	expectedNetwork := kaspaEvidenceNetwork(opts.NetParams)
	if evidence.Network.Kaspa != expectedNetwork {
		return nil, errors.Errorf("evidence Kaspa network mismatch: expected %s, got %s", expectedNetwork, evidence.Network.Kaspa)
	}
	if manifest.Network != expectedNetwork {
		return nil, errors.Errorf("unsigned manifest network mismatch: expected %s, got %s", expectedNetwork, manifest.Network)
	}
	addCheck("proposal network matches local wallet network")

	if err := verifyExitProposalKeys(opts.NetParams, keysFile, evidence, manifest); err != nil {
		return nil, err
	}
	addCheck("local keys derive the evidence custody address")

	if err := verifyEvidenceBundleChecks(evidence); err != nil {
		return nil, err
	}
	addCheck("Igra exit bundle checks and contract preverification are clean")

	if err := verifyEvidenceExitsMatchManifest(evidence.Exits, manifest.Exits); err != nil {
		return nil, err
	}
	addCheck("evidence exits match unsigned Kaspa transaction outputs")

	if err := verifyUnsignedManifestBasics(manifest, verifyReport, proposal, candidate); err != nil {
		return nil, err
	}
	addCheck("unsigned manifest and Foundry verify report are consistent")

	bundleParts, err := server.DecodeTransactionsFromHex(strings.TrimSpace(proposal.UnsignedBundleHex))
	if err != nil {
		return nil, errors.Wrap(err, "failed to decode unsigned bundle hex")
	}
	if len(bundleParts) != 1 {
		return nil, errors.Errorf("expected exactly one PST in exit proposal, got %d", len(bundleParts))
	}
	pst, err := walletserialization.DeserializePartiallySignedTransaction(bundleParts[0])
	if err != nil {
		return nil, errors.Wrap(err, "failed to deserialize PST")
	}
	addCheck("unsigned bundle decodes as one kaspawallet PST")

	expectedPayload, err := verifyKaspaPST(opts.NetParams, pst, bundleParts[0], manifest, proposal)
	if err != nil {
		return nil, err
	}
	txID := consensushashing.TransactionID(pst.Tx).String()
	result.KaspaTxID = txID
	addCheck("decoded PST matches manifest inputs, outputs, payload, fee, and tx id")

	if err := rebuildAndComparePST(opts.NetParams, pst, bundleParts[0], manifest, expectedPayload); err != nil {
		return nil, err
	}
	addCheck("locally rebuilt PST bytes exactly match the proposal")

	if opts.IgraRPCURL != "" {
		if err := verifyIgraRPCState(opts.IgraRPCURL, evidence); err != nil {
			return nil, err
		}
		addCheck("Igra RPC chain, finality, and contract-code checks passed")
	}
	if opts.KaspaRPCURL != "" {
		if err := verifyKaspaRPCState(opts.NetParams, opts.KaspaRPCURL, evidence, manifest); err != nil {
			return nil, err
		}
		addCheck("Kaspa RPC selected UTXOs are live and mature")
	}

	result.OK = true
	return &exitProposalMaterial{
		Proposal:               proposal,
		Evidence:               evidence,
		EvidenceHash:           evidenceHash,
		UnsignedBundleParts:    bundleParts,
		PartiallySignedTx:      pst,
		PartiallySignedTxBytes: bundleParts[0],
		Result:                 result,
	}, nil
}

func proposalCandidateMaterial(
	proposal exitProposalAPI,
	evidence exitProposalEvidence,
) (exitProposalCandidate, error) {
	var origin exitProposalOrigin
	if len(proposal.Origin) != 0 && !bytes.Equal(proposal.Origin, []byte("null")) {
		if err := json.Unmarshal(proposal.Origin, &origin); err != nil {
			return exitProposalCandidate{}, errors.Wrap(err, "failed to decode proposal origin")
		}
		if origin.Candidate.UnsignedManifest.Schema != "" {
			return origin.Candidate, nil
		}
	}

	if evidence.KaspaTransaction.UnsignedManifest.Schema != "" {
		return exitProposalCandidate{
			BuildInput:             evidence.KaspaTransaction.BuildInput,
			UnsignedManifest:       evidence.KaspaTransaction.UnsignedManifest,
			UnsignedVerify:         evidence.KaspaTransaction.UnsignedVerify,
			WalletHexNormalization: evidence.KaspaTransaction.WalletHexNormalization,
		}, nil
	}

	return exitProposalCandidate{}, errors.New("proposal has no candidate unsigned manifest in origin.candidate or legacy evidence.kaspaTransaction")
}

func loadExitProposalAndEvidence(opts exitProposalVerifyOptions) (exitProposalAPI, []byte, error) {
	if opts.ProposalFile != "" {
		proposalBytes, err := os.ReadFile(opts.ProposalFile)
		if err != nil {
			return exitProposalAPI{}, nil, errors.Wrapf(err, "failed to read proposal file %s", opts.ProposalFile)
		}
		evidenceBytes, err := os.ReadFile(opts.EvidenceFile)
		if err != nil {
			return exitProposalAPI{}, nil, errors.Wrapf(err, "failed to read evidence file %s", opts.EvidenceFile)
		}
		var proposal exitProposalAPI
		if err := json.Unmarshal(proposalBytes, &proposal); err != nil {
			return exitProposalAPI{}, nil, errors.Wrap(err, "failed to decode proposal file")
		}
		evidenceRaw, err := extractEvidenceJSON(evidenceBytes)
		if err != nil {
			return exitProposalAPI{}, nil, err
		}
		return proposal, evidenceRaw, nil
	}

	baseURL := strings.TrimRight(opts.SafeURL, "/")
	proposalURL := fmt.Sprintf("%s/api/v1/kaspa/transactions/%s/", baseURL, url.PathEscape(opts.ProposalHash))
	proposalBytes, err := httpGetBytes(proposalURL)
	if err != nil {
		return exitProposalAPI{}, nil, err
	}
	var proposal exitProposalAPI
	if err := json.Unmarshal(proposalBytes, &proposal); err != nil {
		return exitProposalAPI{}, nil, errors.Wrap(err, "failed to decode safe-service proposal response")
	}
	if proposal.ExitBatch == "" {
		return exitProposalAPI{}, nil, errors.New("safe-service proposal is not attached to an exit batch")
	}

	batchURL := fmt.Sprintf("%s/api/v1/kaspa/exit-batches/%s/", baseURL, url.PathEscape(proposal.ExitBatch))
	batchBytes, err := httpGetBytes(batchURL)
	if err != nil {
		return exitProposalAPI{}, nil, err
	}
	evidenceRaw, err := extractEvidenceJSON(batchBytes)
	if err != nil {
		return exitProposalAPI{}, nil, err
	}
	return proposal, evidenceRaw, nil
}

func httpGetBytes(url string) ([]byte, error) {
	client := &http.Client{Timeout: 30 * time.Second}
	response, err := client.Get(url)
	if err != nil {
		return nil, errors.Wrapf(err, "GET %s failed", url)
	}
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	if err != nil {
		return nil, errors.Wrapf(err, "GET %s read failed", url)
	}
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return nil, errors.Errorf("GET %s returned %s: %s", url, response.Status, string(body))
	}
	return body, nil
}

func extractEvidenceJSON(data []byte) ([]byte, error) {
	var probe map[string]json.RawMessage
	if err := json.Unmarshal(data, &probe); err != nil {
		return nil, errors.Wrap(err, "failed to decode evidence JSON")
	}
	if rawKind, ok := probe["kind"]; ok {
		var kind string
		if err := json.Unmarshal(rawKind, &kind); err == nil && kind == exitProposalEvidenceKind {
			return data, nil
		}
	}
	if rawEvidence, ok := probe["evidence"]; ok {
		if len(rawEvidence) == 0 || bytes.Equal(rawEvidence, []byte("null")) {
			return nil, errors.New("exit batch response does not contain evidence")
		}
		return rawEvidence, nil
	}
	return nil, errors.New("input is neither raw exit evidence nor an exit batch response with an evidence field")
}

func canonicalJSONHashBytes(raw []byte) (string, interface{}, error) {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	var value interface{}
	if err := decoder.Decode(&value); err != nil {
		return "", nil, err
	}
	encoded, err := canonicalJSON(value)
	if err != nil {
		return "", nil, err
	}
	hash := sha256.Sum256(encoded)
	return hex.EncodeToString(hash[:]), value, nil
}

func canonicalJSONHashValue(value interface{}) (string, error) {
	encoded, err := canonicalJSON(value)
	if err != nil {
		return "", err
	}
	hash := sha256.Sum256(encoded)
	return hex.EncodeToString(hash[:]), nil
}

func canonicalJSON(value interface{}) ([]byte, error) {
	var buf bytes.Buffer
	encoder := json.NewEncoder(&buf)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(value); err != nil {
		return nil, err
	}
	return bytes.TrimSpace(buf.Bytes()), nil
}

func verifyExitProposalKeys(
	params *dagconfig.Params,
	keysFile *keys.File,
	evidence exitProposalEvidence,
	manifest unsignedExitManifest,
) error {
	if evidence.Bridge.Threshold != keysFile.MinimumSignatures {
		return errors.Errorf("evidence threshold %d does not match local keys threshold %d", evidence.Bridge.Threshold, keysFile.MinimumSignatures)
	}
	if manifest.Multisig.MinimumSignatures != keysFile.MinimumSignatures {
		return errors.Errorf("manifest threshold %d does not match local keys threshold %d", manifest.Multisig.MinimumSignatures, keysFile.MinimumSignatures)
	}
	if evidence.Bridge.ECDSA != keysFile.ECDSA || manifest.Multisig.ECDSA != keysFile.ECDSA {
		return errors.New("proposal ECDSA flag does not match local keys")
	}
	evidenceKeysMatch, err := sameExtendedPublicKeys(evidence.Bridge.Xpubs, keysFile.ExtendedPublicKeys)
	if err != nil {
		return errors.Wrap(err, "failed to compare evidence xpubs")
	}
	if !evidenceKeysMatch {
		return errors.New("evidence xpubs do not match local keys file")
	}
	manifestKeysMatch, err := sameExtendedPublicKeys(manifest.Multisig.ExtendedPublicKeys, keysFile.ExtendedPublicKeys)
	if err != nil {
		return errors.Wrap(err, "failed to compare manifest xpubs")
	}
	if !manifestKeysMatch {
		return errors.New("manifest xpubs do not match local keys file")
	}
	sortedXpubs := sortedStrings(keysFile.ExtendedPublicKeys)
	xpubFingerprint, err := canonicalJSONHashValue(sortedXpubs)
	if err != nil {
		return err
	}
	if evidence.Bridge.XpubFingerprint != "" && evidence.Bridge.XpubFingerprint != xpubFingerprint {
		return errors.Errorf("xpub fingerprint mismatch: evidence has %s, local keys hash to %s", evidence.Bridge.XpubFingerprint, xpubFingerprint)
	}

	derivedAddress, err := libkaspawallet.Address(
		params,
		append([]string(nil), keysFile.ExtendedPublicKeys...),
		keysFile.MinimumSignatures,
		evidence.Bridge.DerivationPath,
		keysFile.ECDSA,
	)
	if err != nil {
		return errors.Wrap(err, "failed to derive custody address from local keys")
	}
	if derivedAddress.String() != evidence.Bridge.Address {
		return errors.Errorf("custody address mismatch: evidence has %s, local keys derive %s", evidence.Bridge.Address, derivedAddress.String())
	}
	if evidence.Bridge.ScriptPublicKey != "" {
		scriptPublicKey, err := txscript.PayToAddrScript(derivedAddress)
		if err != nil {
			return err
		}
		expected, err := scriptPublicKeyFromManifestHex(0, evidence.Bridge.ScriptPublicKey)
		if err != nil {
			return errors.Wrap(err, "invalid bridge script public key in evidence")
		}
		if !scriptPublicKeysEqual(scriptPublicKey, expected) {
			return errors.New("evidence bridge script public key does not match derived custody address")
		}
	}
	return nil
}

func verifyEvidenceBundleChecks(evidence exitProposalEvidence) error {
	globalErrors, ok := evidence.Bundle.Checks["globalErrors"]
	if !ok {
		return errors.New("bundle checks are missing globalErrors")
	}
	if containsNonEmptySlice(globalErrors) {
		return errors.Errorf("bundle globalErrors is not empty: %v", globalErrors)
	}
	if anyCheckFailed(evidence.Bundle.Checks, "metadata", "exit", "totals") != 0 {
		return errors.New("exit bundle reports failed exit checks")
	}
	if anyCheckFailed(evidence.Bundle.Checks, "metadata", "tree", "totals") != 0 {
		return errors.New("exit bundle reports failed tree checks")
	}
	contracts, ok := evidence.Bundle.ContractPreverify["contracts"].([]interface{})
	if !ok || len(contracts) == 0 {
		return errors.New("contract preverify artifact contains no contracts")
	}
	if !allMatchValuesTrue(evidence.Bundle.ContractPreverify) {
		return errors.New("contract preverify artifact contains a non-matching contract or slot")
	}
	return nil
}

func verifyEvidenceExitsMatchManifest(evidenceExits []evidenceExit, manifestExits []manifestExit) error {
	if len(evidenceExits) != len(manifestExits) {
		return errors.Errorf("evidence has %d exits, manifest has %d", len(evidenceExits), len(manifestExits))
	}
	for i := range evidenceExits {
		if !sameHex(evidenceExits[i].MessageID, manifestExits[i].MessageID) {
			return errors.Errorf("exit %d message id mismatch", i)
		}
		if evidenceExits[i].Recipient != manifestExits[i].Recipient {
			return errors.Errorf("exit %d recipient mismatch", i)
		}
		if evidenceExits[i].AmountSompi != manifestExits[i].AmountSompi {
			return errors.Errorf("exit %d amount mismatch", i)
		}
	}
	return nil
}

func verifyUnsignedManifestBasics(
	manifest unsignedExitManifest,
	verifyReport unsignedVerifyReport,
	proposal exitProposalAPI,
	candidate exitProposalCandidate,
) error {
	if manifest.Schema != "igra.exit.unsigned.v1" {
		return errors.Errorf("unsupported unsigned manifest schema %q", manifest.Schema)
	}
	if !sameHex(manifest.Protocol.PayloadHeader, "0x93") {
		return errors.Errorf("unsupported payload header %q", manifest.Protocol.PayloadHeader)
	}
	if manifest.Protocol.KaspaTxID == "" {
		return errors.New("manifest kaspa tx id is empty")
	}
	if manifest.Protocol.TxIDPrefix != "" && !strings.HasPrefix(strings.ToLower(manifest.Protocol.KaspaTxID), strings.TrimPrefix(strings.ToLower(manifest.Protocol.TxIDPrefix), "0x")) {
		return errors.New("manifest Kaspa tx id does not start with configured tx id prefix")
	}
	if verifyReport.OK != true {
		return errors.New("Foundry unsigned verify report is not ok")
	}
	if verifyReport.FullySigned {
		return errors.New("proposal PST is already fully signed")
	}
	if verifyReport.SignedInputs != 0 {
		return errors.Errorf("proposal PST already has %d signed inputs", verifyReport.SignedInputs)
	}
	if verifyReport.KaspaTxID != "" && !sameHex(verifyReport.KaspaTxID, manifest.Protocol.KaspaTxID) {
		return errors.New("verify report tx id does not match manifest tx id")
	}
	if len(manifest.LockingUTXOs) != manifest.Wallet.Inputs {
		return errors.New("manifest wallet input count does not match locking UTXOs")
	}
	expectedOutputs := len(manifest.Exits)
	if manifest.Change != nil && manifest.Change.AmountSompi > 0 {
		expectedOutputs++
	}
	if expectedOutputs != manifest.Wallet.Outputs {
		return errors.New("manifest wallet output count does not match exits plus change")
	}
	unsignedHexHash := sha256.Sum256([]byte(strings.TrimSpace(proposal.UnsignedBundleHex)))
	unsignedHexHashString := hex.EncodeToString(unsignedHexHash[:])
	if manifest.Wallet.HexSha256 != "" && strip0xLower(manifest.Wallet.HexSha256) != unsignedHexHashString {
		if !candidateNormalizedBundleHashMatches(candidate, manifest.Wallet.HexSha256, unsignedHexHashString) {
			return errors.New("manifest wallet.hex_sha256 does not match proposal unsigned bundle hex")
		}
	}
	return nil
}

func candidateNormalizedBundleHashMatches(candidate exitProposalCandidate, originalHexHash, proposalHexHash string) bool {
	normalization := candidate.WalletHexNormalization
	if len(normalization) == 0 {
		return false
	}
	applied, ok := normalizationBool(normalization, "applied")
	if !ok || !applied {
		return false
	}
	normalizedHash := normalizationString(normalization, "normalizedBundleHexSha256", "normalized_bundle_hex_sha256")
	if strip0xLower(normalizedHash) != proposalHexHash {
		return false
	}
	originalHash := normalizationString(normalization, "originalBundleHexSha256", "original_bundle_hex_sha256")
	return originalHash == "" || strip0xLower(originalHash) == strip0xLower(originalHexHash)
}

func verifyKaspaPST(
	params *dagconfig.Params,
	pst *walletserialization.PartiallySignedTransaction,
	pstBytes []byte,
	manifest unsignedExitManifest,
	proposal exitProposalAPI,
) ([]byte, error) {
	tx := pst.Tx
	if len(tx.Inputs) != len(manifest.LockingUTXOs) {
		return nil, errors.Errorf("PST has %d inputs, manifest has %d locking UTXOs", len(tx.Inputs), len(manifest.LockingUTXOs))
	}
	if len(pst.PartiallySignedInputs) != len(manifest.LockingUTXOs) {
		return nil, errors.Errorf("PST has %d partial inputs, manifest has %d locking UTXOs", len(pst.PartiallySignedInputs), len(manifest.LockingUTXOs))
	}

	expectedPayments, err := expectedManifestPayments(manifest)
	if err != nil {
		return nil, err
	}
	if len(tx.Outputs) != len(expectedPayments) {
		return nil, errors.Errorf("PST has %d outputs, manifest expects %d", len(tx.Outputs), len(expectedPayments))
	}

	payloadFromManifest, err := decodeHexField(manifest.Protocol.PayloadHex)
	if err != nil {
		return nil, errors.Wrap(err, "invalid manifest payload_hex")
	}
	payloadFromExits, err := buildIgraExitPayload(manifest.Exits, manifest.Protocol.Nonce)
	if err != nil {
		return nil, err
	}
	if !bytes.Equal(payloadFromManifest, payloadFromExits) {
		return nil, errors.New("manifest payload_hex does not match message IDs and nonce")
	}
	if !bytes.Equal(tx.Payload, payloadFromManifest) {
		return nil, errors.New("PST transaction payload does not match manifest payload")
	}

	for i, manifestUTXO := range manifest.LockingUTXOs {
		expectedOutpoint, expectedPrevOutput, err := manifestUTXOToDomain(manifestUTXO)
		if err != nil {
			return nil, errors.Wrapf(err, "invalid manifest locking UTXO %d", i)
		}
		if tx.Inputs[i].PreviousOutpoint != *expectedOutpoint {
			return nil, errors.Errorf("input %d outpoint mismatch", i)
		}
		if len(tx.Inputs[i].SignatureScript) != 0 {
			return nil, errors.Errorf("input %d already has a signature script", i)
		}
		partialInput := pst.PartiallySignedInputs[i]
		if partialInput.DerivationPath != manifestUTXO.DerivationPath {
			return nil, errors.Errorf("partial input %d derivation path mismatch", i)
		}
		if partialInput.MinimumSignatures != manifest.Multisig.MinimumSignatures {
			return nil, errors.Errorf("partial input %d threshold mismatch", i)
		}
		if len(partialInput.PubKeySignaturePairs) != len(manifest.Multisig.ExtendedPublicKeys) {
			return nil, errors.Errorf("partial input %d xpub slot count mismatch", i)
		}
		for pairIndex, pair := range partialInput.PubKeySignaturePairs {
			if len(pair.Signature) != 0 {
				return nil, errors.Errorf("partial input %d xpub slot %d is already signed", i, pairIndex)
			}
		}
		if !scriptPublicKeysEqual(partialInput.PrevOutput.ScriptPublicKey, expectedPrevOutput.ScriptPublicKey) ||
			partialInput.PrevOutput.Value != expectedPrevOutput.Value {
			return nil, errors.Errorf("partial input %d prev output mismatch", i)
		}
	}

	totalOutput := uint64(0)
	for i, expectedPayment := range expectedPayments {
		addr, err := util.DecodeAddress(expectedPayment.Address, params.Prefix)
		if err != nil {
			return nil, errors.Wrapf(err, "invalid output address %s", expectedPayment.Address)
		}
		expectedScript, err := txscript.PayToAddrScript(addr)
		if err != nil {
			return nil, err
		}
		if tx.Outputs[i].Value != expectedPayment.Amount {
			return nil, errors.Errorf("output %d amount mismatch", i)
		}
		if !scriptPublicKeysEqual(tx.Outputs[i].ScriptPublicKey, expectedScript) {
			return nil, errors.Errorf("output %d script public key mismatch", i)
		}
		totalOutput += expectedPayment.Amount
	}

	totalInput := uint64(0)
	for _, manifestUTXO := range manifest.LockingUTXOs {
		totalInput += manifestUTXO.AmountSompi
	}
	if totalInput != manifest.TotalInputSompi {
		return nil, errors.New("manifest total input does not match locking UTXOs")
	}
	if totalOutput != manifest.TotalOutputSompi {
		return nil, errors.New("manifest total output does not match outputs")
	}
	if totalInput < totalOutput {
		return nil, errors.New("manifest outputs exceed inputs")
	}
	if totalInput-totalOutput != manifest.FeeSompi {
		return nil, errors.New("manifest fee does not equal inputs minus outputs")
	}
	if proposal.FeeSompi != nil && *proposal.FeeSompi != manifest.FeeSompi {
		return nil, errors.New("proposal fee_sompi does not match manifest fee")
	}

	txID := consensushashing.TransactionID(tx).String()
	if !sameHex(txID, manifest.Protocol.KaspaTxID) {
		return nil, errors.Errorf("PST tx id %s does not match manifest tx id %s", txID, manifest.Protocol.KaspaTxID)
	}
	if len(proposal.TxIDs) > 0 && !sameHex(proposal.TxIDs[0], txID) {
		return nil, errors.Errorf("proposal tx_ids[0] %s does not match PST tx id %s", proposal.TxIDs[0], txID)
	}
	if len(pstBytes) == 0 {
		return nil, errors.New("PST bytes are empty")
	}
	return payloadFromManifest, nil
}

func rebuildAndComparePST(
	params *dagconfig.Params,
	pst *walletserialization.PartiallySignedTransaction,
	pstBytes []byte,
	manifest unsignedExitManifest,
	payload []byte,
) error {
	expectedPayments, err := expectedManifestPayments(manifest)
	if err != nil {
		return err
	}
	payments := make([]*libkaspawallet.Payment, len(expectedPayments))
	for i, expected := range expectedPayments {
		addr, err := util.DecodeAddress(expected.Address, params.Prefix)
		if err != nil {
			return err
		}
		payments[i] = &libkaspawallet.Payment{Address: addr, Amount: expected.Amount}
	}

	selectedUTXOs := make([]*libkaspawallet.UTXO, len(manifest.LockingUTXOs))
	for i, manifestUTXO := range manifest.LockingUTXOs {
		outpoint, prevOutput, err := manifestUTXOToDomain(manifestUTXO)
		if err != nil {
			return err
		}
		selectedUTXOs[i] = &libkaspawallet.UTXO{
			Outpoint: outpoint,
			UTXOEntry: utxo.NewUTXOEntry(
				prevOutput.Value,
				prevOutput.ScriptPublicKey,
				false,
				0,
			),
			DerivationPath: manifestUTXO.DerivationPath,
		}
	}

	rebuilt, err := libkaspawallet.CreateUnsignedTransaction(
		append([]string(nil), manifest.Multisig.ExtendedPublicKeys...),
		manifest.Multisig.MinimumSignatures,
		payments,
		selectedUTXOs,
	)
	if err != nil {
		return errors.Wrap(err, "failed to locally rebuild PST")
	}
	rebuilt.Tx.Payload = append([]byte(nil), payload...)
	rebuiltBytes, err := walletserialization.SerializePartiallySignedTransaction(rebuilt)
	if err != nil {
		return errors.Wrap(err, "failed to serialize locally rebuilt PST")
	}
	if !bytes.Equal(rebuiltBytes, pstBytes) {
		if consensushashing.TransactionID(rebuilt.Tx).String() == consensushashing.TransactionID(pst.Tx).String() {
			return errors.New("rebuilt PST has the same transaction id but different serialized PST bytes")
		}
		return errors.New("rebuilt PST does not match proposed PST")
	}
	return nil
}

func expectedManifestPayments(manifest unsignedExitManifest) ([]expectedPayment, error) {
	payments := make([]expectedPayment, 0, len(manifest.Exits)+1)
	for _, exit := range manifest.Exits {
		payments = append(payments, expectedPayment{
			Address: exit.Recipient,
			Amount:  exit.AmountSompi,
		})
	}
	if manifest.Change != nil && manifest.Change.AmountSompi > 0 {
		payments = append(payments, expectedPayment{
			Address: manifest.Change.Address,
			Amount:  manifest.Change.AmountSompi,
		})
	}
	return payments, nil
}

func manifestUTXOToDomain(manifestUTXO manifestUTXO) (*externalapi.DomainOutpoint, *externalapi.DomainTransactionOutput, error) {
	txID, err := externalapi.NewDomainTransactionIDFromString(manifestUTXO.TransactionID)
	if err != nil {
		return nil, nil, err
	}
	scriptPublicKey, err := scriptPublicKeyFromManifestHex(
		manifestUTXO.ScriptPublicKey.Version,
		manifestUTXO.ScriptPublicKey.Script,
	)
	if err != nil {
		return nil, nil, err
	}
	return &externalapi.DomainOutpoint{
			TransactionID: *txID,
			Index:         manifestUTXO.Index,
		},
		&externalapi.DomainTransactionOutput{
			Value:           manifestUTXO.AmountSompi,
			ScriptPublicKey: scriptPublicKey,
		},
		nil
}

func scriptPublicKeyFromManifestHex(version uint16, scriptHex string) (*externalapi.ScriptPublicKey, error) {
	script, err := decodeHexField(scriptHex)
	if err != nil {
		return nil, err
	}
	return &externalapi.ScriptPublicKey{
		Version: version,
		Script:  script,
	}, nil
}

func buildIgraExitPayload(exits []manifestExit, nonceHex string) ([]byte, error) {
	payload := []byte{0x93}
	for i, exit := range exits {
		messageID, err := decodeHexField(exit.MessageID)
		if err != nil {
			return nil, errors.Wrapf(err, "invalid exit %d message_id", i)
		}
		if len(messageID) != 32 {
			return nil, errors.Errorf("exit %d message_id is %d bytes, expected 32", i, len(messageID))
		}
		payload = append(payload, messageID...)
	}
	nonce, err := strconv.ParseUint(strip0xLower(nonceHex), 16, 32)
	if err != nil {
		return nil, errors.Wrap(err, "invalid payload nonce")
	}
	var nonceBytes [4]byte
	binary.BigEndian.PutUint32(nonceBytes[:], uint32(nonce))
	payload = append(payload, nonceBytes[:]...)
	return payload, nil
}

func verifyIgraRPCState(rpcURL string, evidence exitProposalEvidence) error {
	chainIDHex, err := ethRPCString(rpcURL, "eth_chainId", []interface{}{})
	if err != nil {
		return err
	}
	chainID, err := strconv.ParseUint(strip0xLower(chainIDHex), 16, 64)
	if err != nil {
		return errors.Wrap(err, "invalid eth_chainId result")
	}
	if chainID != evidence.Network.IgraChainID {
		return errors.Errorf("Igra chain id mismatch: RPC has %d, evidence has %d", chainID, evidence.Network.IgraChainID)
	}

	finalizedBlock, err := ethRPCBlockNumber(rpcURL, "finalized")
	if err != nil {
		return err
	}
	if finalizedBlock < evidence.Window.FinalizedAtBlock {
		return errors.Errorf("Igra finalized block %d is behind evidence finalized block %d", finalizedBlock, evidence.Window.FinalizedAtBlock)
	}

	for name, address := range map[string]string{
		"kasExitBridge":  evidence.Contracts.KasExitBridge,
		"mailbox":        evidence.Contracts.Mailbox,
		"merkleTreeHook": evidence.Contracts.MerkleTreeHook,
	} {
		if address == "" {
			return errors.Errorf("evidence contract %s address is empty", name)
		}
		code, err := ethRPCString(rpcURL, "eth_getCode", []interface{}{address, "latest"})
		if err != nil {
			return err
		}
		if code == "" || code == "0x" {
			return errors.Errorf("Igra RPC has no code for %s at %s", name, address)
		}
	}
	return nil
}

func ethRPCBlockNumber(rpcURL string, tag string) (uint64, error) {
	block, err := ethRPCObject(rpcURL, "eth_getBlockByNumber", []interface{}{tag, false})
	if err != nil {
		return 0, err
	}
	numberValue, ok := block["number"].(string)
	if !ok || numberValue == "" {
		return 0, errors.Errorf("eth_getBlockByNumber(%s) did not return a block number", tag)
	}
	number, err := strconv.ParseUint(strip0xLower(numberValue), 16, 64)
	if err != nil {
		return 0, errors.Wrapf(err, "invalid block number for tag %s", tag)
	}
	return number, nil
}

func ethRPCString(rpcURL, method string, params []interface{}) (string, error) {
	result, err := ethRPC(rpcURL, method, params)
	if err != nil {
		return "", err
	}
	value, ok := result.(string)
	if !ok {
		return "", errors.Errorf("%s returned %T, expected string", method, result)
	}
	return value, nil
}

func ethRPCObject(rpcURL, method string, params []interface{}) (map[string]interface{}, error) {
	result, err := ethRPC(rpcURL, method, params)
	if err != nil {
		return nil, err
	}
	value, ok := result.(map[string]interface{})
	if !ok {
		return nil, errors.Errorf("%s returned %T, expected object", method, result)
	}
	return value, nil
}

func ethRPC(rpcURL, method string, params []interface{}) (interface{}, error) {
	requestBody, err := json.Marshal(map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      1,
		"method":  method,
		"params":  params,
	})
	if err != nil {
		return nil, err
	}
	client := &http.Client{Timeout: 30 * time.Second}
	response, err := client.Post(rpcURL, "application/json", bytes.NewReader(requestBody))
	if err != nil {
		return nil, errors.Wrapf(err, "Igra RPC %s failed", method)
	}
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	if err != nil {
		return nil, err
	}
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return nil, errors.Errorf("Igra RPC %s returned %s: %s", method, response.Status, string(body))
	}
	var rpcResponse struct {
		Result interface{} `json:"result"`
		Error  *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal(body, &rpcResponse); err != nil {
		return nil, errors.Wrapf(err, "failed to decode Igra RPC %s response", method)
	}
	if rpcResponse.Error != nil {
		return nil, errors.Errorf("Igra RPC %s error %d: %s", method, rpcResponse.Error.Code, rpcResponse.Error.Message)
	}
	return rpcResponse.Result, nil
}

func verifyKaspaRPCState(
	params *dagconfig.Params,
	rpcURL string,
	evidence exitProposalEvidence,
	manifest unsignedExitManifest,
) error {
	rpcAddress := normalizeKaspaRPCAddress(rpcURL)
	client, err := rpcclient.NewRPCClient(rpcAddress)
	if err != nil {
		return err
	}
	defer client.Disconnect()

	dagInfo, err := client.GetBlockDAGInfo()
	if err != nil {
		return err
	}
	if dagInfo.NetworkName != "" && dagInfo.NetworkName != params.Name {
		return errors.Errorf("Kaspa RPC network mismatch: RPC has %s, local wallet uses %s", dagInfo.NetworkName, params.Name)
	}

	utxos, err := client.GetUTXOsByAddresses([]string{evidence.Bridge.Address})
	if err != nil {
		return err
	}
	live := make(map[string]*appmessage.UTXOsByAddressesEntry)
	for _, entry := range utxos.Entries {
		if entry == nil || entry.Outpoint == nil || entry.UTXOEntry == nil {
			continue
		}
		key := outpointKey(entry.Outpoint.TransactionID, entry.Outpoint.Index)
		live[key] = entry
	}

	for _, manifestUTXO := range manifest.LockingUTXOs {
		entry := live[outpointKey(manifestUTXO.TransactionID, manifestUTXO.Index)]
		if entry == nil {
			return errors.Errorf("selected UTXO %s:%d is not live at custody address %s", manifestUTXO.TransactionID, manifestUTXO.Index, evidence.Bridge.Address)
		}
		if entry.UTXOEntry.Amount != manifestUTXO.AmountSompi {
			return errors.Errorf("selected UTXO %s:%d amount mismatch", manifestUTXO.TransactionID, manifestUTXO.Index)
		}
		if entry.UTXOEntry.ScriptPublicKey == nil {
			return errors.Errorf("selected UTXO %s:%d has no script public key", manifestUTXO.TransactionID, manifestUTXO.Index)
		}
		if entry.UTXOEntry.ScriptPublicKey.Version != manifestUTXO.ScriptPublicKey.Version ||
			!sameHex(entry.UTXOEntry.ScriptPublicKey.Script, manifestUTXO.ScriptPublicKey.Script) {
			return errors.Errorf("selected UTXO %s:%d script mismatch", manifestUTXO.TransactionID, manifestUTXO.Index)
		}
		if entry.UTXOEntry.IsCoinbase {
			maturesAt := entry.UTXOEntry.BlockDAAScore + params.BlockCoinbaseMaturity
			if dagInfo.VirtualDAAScore < maturesAt {
				return errors.Errorf("selected coinbase UTXO %s:%d is immature: current DAA %d, matures at %d", manifestUTXO.TransactionID, manifestUTXO.Index, dagInfo.VirtualDAAScore, maturesAt)
			}
		}
	}
	return nil
}

func normalizeKaspaRPCAddress(rpcURL string) string {
	out := strings.TrimSpace(rpcURL)
	out = strings.TrimPrefix(out, "grpc://")
	out = strings.TrimPrefix(out, "http://")
	out = strings.TrimPrefix(out, "https://")
	return strings.TrimRight(out, "/")
}

func printExitProposalVerification(result exitProposalVerificationResult, asJSON bool) error {
	if asJSON {
		encoded, err := json.MarshalIndent(result, "", "  ")
		if err != nil {
			return err
		}
		fmt.Println(string(encoded))
		return nil
	}
	fmt.Printf("Verified proposal %s\n", result.ProposalHash)
	fmt.Printf("Evidence hash: %s\n", result.EvidenceHash)
	fmt.Printf("Kaspa tx id:   %s\n", result.KaspaTxID)
	for _, check := range result.Checks {
		fmt.Printf("  OK %s\n", check)
	}
	return nil
}

func kaspaEvidenceNetwork(params *dagconfig.Params) string {
	switch params.Name {
	case "kaspa-mainnet":
		return "mainnet"
	case "kaspa-testnet-10":
		return "testnet"
	case "kaspa-devnet":
		return "devnet"
	case "kaspa-simnet":
		return "simnet"
	default:
		return params.Name
	}
}

func decodeHexField(value string) ([]byte, error) {
	return hex.DecodeString(strip0xLower(value))
}

func strip0xLower(value string) string {
	value = strings.TrimSpace(strings.ToLower(value))
	return strings.TrimPrefix(value, "0x")
}

func normalizationString(normalization map[string]interface{}, keys ...string) string {
	for _, key := range keys {
		value, ok := normalization[key]
		if !ok {
			continue
		}
		if typed, ok := value.(string); ok {
			return typed
		}
	}
	return ""
}

func normalizationBool(normalization map[string]interface{}, key string) (bool, bool) {
	value, ok := normalization[key]
	if !ok {
		return false, false
	}
	typed, ok := value.(bool)
	return typed, ok
}

func sameHex(left, right string) bool {
	return strip0xLower(left) == strip0xLower(right)
}

func sameSortedStrings(left, right []string) bool {
	leftCopy := sortedStrings(left)
	rightCopy := sortedStrings(right)
	if len(leftCopy) != len(rightCopy) {
		return false
	}
	for i := range leftCopy {
		if leftCopy[i] != rightCopy[i] {
			return false
		}
	}
	return true
}

func sameExtendedPublicKeys(left, right []string) (bool, error) {
	leftNormalized, err := normalizedExtendedPublicKeys(left)
	if err != nil {
		return false, err
	}
	rightNormalized, err := normalizedExtendedPublicKeys(right)
	if err != nil {
		return false, err
	}
	return sameSortedStrings(leftNormalized, rightNormalized), nil
}

func normalizedExtendedPublicKeys(in []string) ([]string, error) {
	out := make([]string, len(in))
	for i, key := range in {
		identity, err := extendedPublicKeyIdentity(key)
		if err != nil {
			return nil, err
		}
		out[i] = identity
	}
	sort.Strings(out)
	return out, nil
}

func extendedPublicKeyIdentity(key string) (string, error) {
	extendedKey, err := bip32.DeserializeExtendedKey(key)
	if err != nil {
		return "", errors.Wrap(err, "failed to decode extended public key")
	}
	publicKey, err := extendedKey.PublicKey()
	if err != nil {
		return "", err
	}
	serializedPublicKey, err := publicKey.Serialize()
	if err != nil {
		return "", err
	}

	return fmt.Sprintf(
		"depth=%d,parent=%x,child=%d,chain=%x,pub=%x",
		extendedKey.Depth,
		extendedKey.ParentFingerprint,
		extendedKey.ChildNumber,
		extendedKey.ChainCode,
		serializedPublicKey,
	), nil
}

func sortedStrings(in []string) []string {
	out := append([]string(nil), in...)
	sort.Strings(out)
	return out
}

func scriptPublicKeysEqual(left, right *externalapi.ScriptPublicKey) bool {
	if left == nil || right == nil {
		return left == right
	}
	return left.Version == right.Version && bytes.Equal(left.Script, right.Script)
}

func outpointKey(txID string, index uint32) string {
	return strings.ToLower(txID) + ":" + strconv.FormatUint(uint64(index), 10)
}

func containsNonEmptySlice(value interface{}) bool {
	switch typed := value.(type) {
	case []interface{}:
		return len(typed) > 0
	case map[string]interface{}:
		for _, child := range typed {
			if containsNonEmptySlice(child) {
				return true
			}
		}
	}
	return false
}

func anyCheckFailed(root map[string]interface{}, path ...string) uint64 {
	value := nestedValue(root, path...)
	totals, ok := value.(map[string]interface{})
	if !ok {
		return 1
	}
	return uint64FromInterface(totals["anyCheckFailed"], 1)
}

func nestedValue(root map[string]interface{}, path ...string) interface{} {
	var current interface{} = root
	for _, element := range path {
		asMap, ok := current.(map[string]interface{})
		if !ok {
			return nil
		}
		current = asMap[element]
	}
	return current
}

func uint64FromInterface(value interface{}, fallback uint64) uint64 {
	switch typed := value.(type) {
	case json.Number:
		parsed, err := typed.Int64()
		if err != nil || parsed < 0 {
			return fallback
		}
		return uint64(parsed)
	case float64:
		if typed < 0 {
			return fallback
		}
		return uint64(typed)
	case int:
		if typed < 0 {
			return fallback
		}
		return uint64(typed)
	case uint64:
		return typed
	case string:
		parsed, err := strconv.ParseUint(typed, 10, 64)
		if err != nil {
			return fallback
		}
		return parsed
	default:
		return fallback
	}
}

func allMatchValuesTrue(value interface{}) bool {
	switch typed := value.(type) {
	case map[string]interface{}:
		for key, child := range typed {
			if key == "allMatch" {
				value, ok := child.(bool)
				if !ok || !value {
					return false
				}
				continue
			}
			if !allMatchValuesTrue(child) {
				return false
			}
		}
	case []interface{}:
		for _, child := range typed {
			if !allMatchValuesTrue(child) {
				return false
			}
		}
	}
	return true
}
