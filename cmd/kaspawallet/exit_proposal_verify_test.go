package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/kaspanet/kaspad/cmd/kaspawallet/daemon/server"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet"
	walletserialization "github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet/serialization"
	"github.com/kaspanet/kaspad/domain/consensus/model/externalapi"
	"github.com/kaspanet/kaspad/domain/consensus/utils/consensushashing"
	"github.com/kaspanet/kaspad/domain/consensus/utils/txscript"
	"github.com/kaspanet/kaspad/domain/consensus/utils/utxo"
	"github.com/kaspanet/kaspad/domain/dagconfig"
	"github.com/kaspanet/kaspad/util"
)

func TestCanonicalJSONHashBytesMatchesSafeServiceEncoding(t *testing.T) {
	hash, _, err := canonicalJSONHashBytes([]byte(`{"b":2,"a":{"d":4,"c":3}}`))
	if err != nil {
		t.Fatalf("canonicalJSONHashBytes: %s", err)
	}

	const expected = "c461c47a913352f1a21e3f2ea49e1fd34754c0dc12cb7366e4636d5e186c6c6e"
	if hash != expected {
		t.Fatalf("hash mismatch: got %s, want %s", hash, expected)
	}
}

func TestBuildIgraExitPayload(t *testing.T) {
	payload, err := buildIgraExitPayload(
		[]manifestExit{
			{MessageID: "0x00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"},
			{MessageID: "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100"},
		},
		"0x0000ed79",
	)
	if err != nil {
		t.Fatalf("buildIgraExitPayload: %s", err)
	}

	const expected = "9300112233445566778899aabbccddeeff00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100ffeeddccbbaa998877665544332211000000ed79"
	if strip0xLower(expected) != strip0xLower(toHex(payload)) {
		t.Fatalf("payload mismatch: got %s, want %s", toHex(payload), expected)
	}
}

func TestVerifyExitProposalWithGeneratedFixture(t *testing.T) {
	params := &dagconfig.MainnetParams
	xpubs := []string{
		"kpub2HoLSHkWgT8VxmjL7Qv2hbh5Jq9h11XmmPmy3ua2QH89iVNzv6W55ZLy4dVAV3ArUMEAFZWmdADauHTbLCGQ54HyBqgeKTjB3Mdv8kxjetC",
		"kpub2HsAfqNwGLzHhbbmGfAHtkgMM26VfqKsqKuCDAMAw4SMAoUx8YpoKcYq9tBCBqJXirASDtqo3iwcQtSF9d2MKCuLbuPPzTgyH8C5dMMb5Ms",
		"kpub2JeC9uSRRMjr2ExKPtB7UJJo134UFwg6MaToXQefhJv2tgvx4aWah7UfbGM72iF2gpxgHSUGBVu7J5a5wnnrQuAqHNusi9i35XwHfKZgmnr",
	}
	const threshold = uint32(2)
	const derivationPath = "m/0/0/1"

	custodyAddress, err := libkaspawallet.Address(params, append([]string(nil), xpubs...), threshold, derivationPath, false)
	if err != nil {
		t.Fatalf("Address: %s", err)
	}
	custodyScript, err := txscript.PayToAddrScript(custodyAddress)
	if err != nil {
		t.Fatalf("PayToAddrScript: %s", err)
	}

	const inputAmount = uint64(1_000_000_000)
	const exitAmount = uint64(900_000_000)
	const feeSompi = uint64(1_000)
	changeAmount := inputAmount - exitAmount - feeSompi
	recipient := "kaspa:qpmv34zy04kwp79fkjnu39gs26me64u3v7zdtf5xqn2rfeztg5ttx5mzzhcq7"
	messageID := "0x00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
	nonce := "0x0000002a"
	payload, err := buildIgraExitPayload([]manifestExit{{MessageID: messageID, Recipient: recipient, AmountSompi: exitAmount}}, nonce)
	if err != nil {
		t.Fatalf("buildIgraExitPayload: %s", err)
	}

	txID, err := externalapi.NewDomainTransactionIDFromString("97b1" + strings.Repeat("00", 30))
	if err != nil {
		t.Fatalf("NewDomainTransactionIDFromString: %s", err)
	}
	selectedUTXO := &libkaspawallet.UTXO{
		Outpoint: &externalapi.DomainOutpoint{TransactionID: *txID, Index: 0},
		UTXOEntry: utxo.NewUTXOEntry(
			inputAmount,
			custodyScript,
			false,
			0,
		),
		DerivationPath: derivationPath,
	}
	recipientAddress, err := util.DecodeAddress(recipient, params.Prefix)
	if err != nil {
		t.Fatalf("DecodeAddress recipient: %s", err)
	}
	unsignedPST, err := libkaspawallet.CreateUnsignedTransaction(
		append([]string(nil), xpubs...),
		threshold,
		[]*libkaspawallet.Payment{
			{Address: recipientAddress, Amount: exitAmount},
			{Address: custodyAddress, Amount: changeAmount},
		},
		[]*libkaspawallet.UTXO{selectedUTXO},
	)
	if err != nil {
		t.Fatalf("CreateUnsignedTransaction: %s", err)
	}
	unsignedPST.Tx.Payload = payload
	unsignedPSTBytes, err := walletserialization.SerializePartiallySignedTransaction(unsignedPST)
	if err != nil {
		t.Fatalf("SerializePartiallySignedTransaction: %s", err)
	}
	unsignedBundleHex := server.EncodeTransactionsToHex([][]byte{unsignedPSTBytes})
	bundleHash := sha256.Sum256([]byte(unsignedBundleHex))
	kaspaTxID := consensushashing.TransactionID(unsignedPST.Tx).String()

	var manifest unsignedExitManifest
	manifest.Schema = "igra.exit.unsigned.v1"
	manifest.Network = "mainnet"
	manifest.Protocol.PayloadHeader = "0x93"
	manifest.Protocol.TxIDPrefix = "0x" + kaspaTxID[:4]
	manifest.Protocol.Nonce = nonce
	manifest.Protocol.KaspaTxID = kaspaTxID
	manifest.Protocol.PayloadHex = "0x" + hex.EncodeToString(payload)
	manifest.LockingUTXOs = []manifestUTXO{{
		TransactionID:  txID.String(),
		Index:          0,
		AmountSompi:    inputAmount,
		Address:        custodyAddress.String(),
		DerivationPath: derivationPath,
	}}
	manifest.LockingUTXOs[0].ScriptPublicKey.Version = custodyScript.Version
	manifest.LockingUTXOs[0].ScriptPublicKey.Script = hex.EncodeToString(custodyScript.Script)
	manifest.Exits = []manifestExit{{MessageID: messageID, Recipient: recipient, AmountSompi: exitAmount}}
	manifest.Change = &manifestChange{DerivationPath: derivationPath, AmountSompi: changeAmount, Address: custodyAddress.String()}
	manifest.FeeSompi = feeSompi
	manifest.TotalInputSompi = inputAmount
	manifest.TotalOutputSompi = exitAmount + changeAmount
	manifest.Multisig.MinimumSignatures = threshold
	manifest.Multisig.ExtendedPublicKeys = xpubs
	manifest.Multisig.ECDSA = false
	manifest.Wallet.Format = "kaspawallet.PartiallySignedTransaction.hex"
	manifest.Wallet.HexSha256 = "0x" + hex.EncodeToString(bundleHash[:])
	manifest.Wallet.Inputs = 1
	manifest.Wallet.Outputs = 2

	xpubFingerprint, err := canonicalJSONHashValue(sortedStrings(xpubs))
	if err != nil {
		t.Fatalf("canonicalJSONHashValue: %s", err)
	}
	evidence := exitProposalEvidence{
		SchemaVersion: 1,
		Kind:          exitProposalEvidenceKind,
		Exits: []evidenceExit{{
			RequestID:   1,
			MessageID:   messageID,
			Recipient:   recipient,
			AmountSompi: exitAmount,
		}},
	}
	evidence.Network.Kaspa = "mainnet"
	evidence.Network.IgraChainID = 38833
	evidence.Window.FromBlock = 100
	evidence.Window.ToBlock = 200
	evidence.Window.FinalizedAtBlock = 200
	evidence.Contracts.KasExitBridge = "0x0000000000000000000000000000000000000001"
	evidence.Contracts.Mailbox = "0x0000000000000000000000000000000000000002"
	evidence.Contracts.MerkleTreeHook = "0x0000000000000000000000000000000000000003"
	evidence.Bridge.Address = custodyAddress.String()
	evidence.Bridge.ScriptPublicKey = hex.EncodeToString(custodyScript.Script)
	evidence.Bridge.DerivationPath = derivationPath
	evidence.Bridge.Threshold = threshold
	evidence.Bridge.XpubFingerprint = xpubFingerprint
	evidence.Bridge.Xpubs = xpubs
	evidence.Bundle.Checks = map[string]interface{}{
		"globalErrors": map[string]interface{}{"exit": []interface{}{}, "tree": []interface{}{}},
		"metadata": map[string]interface{}{
			"exit": map[string]interface{}{"totals": map[string]interface{}{"anyCheckFailed": float64(0)}},
			"tree": map[string]interface{}{"totals": map[string]interface{}{"anyCheckFailed": float64(0)}},
		},
	}
	evidence.Bundle.ContractPreverify = map[string]interface{}{
		"contracts": []interface{}{
			map[string]interface{}{"allMatch": true, "start": map[string]interface{}{"allMatch": true}, "end": map[string]interface{}{"allMatch": true}},
		},
	}
	candidate := exitProposalCandidate{}
	candidate.UnsignedManifest = manifest
	candidate.UnsignedVerify = unsignedVerifyReport{
		OK:          true,
		KaspaTxID:   kaspaTxID,
		Inputs:      1,
		Outputs:     2,
		FullySigned: false,
	}

	evidenceBytes := mustJSON(t, evidence)
	evidenceHash, _, err := canonicalJSONHashBytes(evidenceBytes)
	if err != nil {
		t.Fatalf("canonicalJSONHashBytes evidence: %s", err)
	}
	proposal := exitProposalAPI{
		ProposalHash:      strings.Repeat("a", 64),
		Format:            "kaspawallet_pst_v1",
		UnsignedBundleHex: unsignedBundleHex,
		ExitEvidenceHash:  evidenceHash,
		TxIDs:             []string{kaspaTxID},
		FeeSompi:          uint64Pointer(feeSompi),
	}
	proposalOrigin := exitProposalOrigin{
		Kind:         "igra-l2-exit",
		EvidenceHash: evidenceHash,
		Candidate:    candidate,
	}
	proposal.Origin = mustJSON(t, proposalOrigin)

	tempDir := t.TempDir()
	keysPath := filepath.Join(tempDir, "keys.json")
	proposalPath := filepath.Join(tempDir, "proposal.json")
	evidencePath := filepath.Join(tempDir, "evidence.json")
	writeJSONFile(t, keysPath, map[string]interface{}{
		"version":               1,
		"encryptedMnemonics":    []interface{}{},
		"publicKeys":            xpubs,
		"minimumSignatures":     threshold,
		"cosignerIndex":         0,
		"lastUsedExternalIndex": 0,
		"lastUsedInternalIndex": 0,
		"ecdsa":                 false,
	})
	writeJSONFile(t, proposalPath, proposal)
	if err := os.WriteFile(evidencePath, evidenceBytes, 0o600); err != nil {
		t.Fatalf("WriteFile evidence: %s", err)
	}

	material, err := verifyExitProposalWithOptions(exitProposalVerifyOptions{
		KeysFile:     keysPath,
		ProposalFile: proposalPath,
		EvidenceFile: evidencePath,
		NetParams:    params,
	})
	if err != nil {
		t.Fatalf("verifyExitProposalWithOptions: %s", err)
	}
	if !material.Result.OK {
		t.Fatalf("verification result is not ok")
	}
	if material.Result.KaspaTxID != kaspaTxID {
		t.Fatalf("tx id mismatch: got %s, want %s", material.Result.KaspaTxID, kaspaTxID)
	}
}

func toHex(data []byte) string {
	const alphabet = "0123456789abcdef"
	out := make([]byte, len(data)*2)
	for i, value := range data {
		out[i*2] = alphabet[value>>4]
		out[i*2+1] = alphabet[value&0x0f]
	}
	return string(out)
}

func uint64Pointer(value uint64) *uint64 {
	return &value
}

func mustJSON(t *testing.T, value interface{}) []byte {
	t.Helper()
	encoded, err := json.Marshal(value)
	if err != nil {
		t.Fatalf("Marshal: %s", err)
	}
	return encoded
}

func writeJSONFile(t *testing.T, path string, value interface{}) {
	t.Helper()
	if err := os.WriteFile(path, mustJSON(t, value), 0o600); err != nil {
		t.Fatalf("WriteFile %s: %s", path, err)
	}
}
