package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"sort"
	"strings"

	"github.com/kaspanet/kaspad/app/appmessage"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet/bip32"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet/serialization"
	"github.com/kaspanet/kaspad/domain/consensus/model/externalapi"
	"github.com/kaspanet/kaspad/domain/consensus/utils/consensushashing"
	"github.com/kaspanet/kaspad/domain/consensus/utils/txscript"
	"github.com/kaspanet/kaspad/domain/dagconfig"
	"github.com/kaspanet/kaspad/infrastructure/network/rpcclient"
	"github.com/pkg/errors"
)

type inspectRequest struct {
	Network         string   `json:"network"`
	BundleHex       string   `json:"bundleHex"`
	FederationXpubs []string `json:"federationXpubs"`
	Threshold       uint32   `json:"threshold"`
	ECDSA           bool     `json:"ecdsa"`
}

type mergeRequest struct {
	Network          string   `json:"network"`
	CurrentBundleHex string   `json:"currentBundleHex"`
	SignedBundleHex  string   `json:"signedBundleHex"`
	FederationXpubs  []string `json:"federationXpubs"`
	Threshold        uint32   `json:"threshold"`
	ECDSA            bool     `json:"ecdsa"`
}

type broadcastRequest struct {
	Network         string   `json:"network"`
	BundleHex       string   `json:"bundleHex"`
	RPCURL          string   `json:"rpcUrl"`
	FederationXpubs []string `json:"federationXpubs"`
	Threshold       uint32   `json:"threshold"`
	ECDSA           bool     `json:"ecdsa"`
}

type inspectResponse struct {
	ProposalHash        string       `json:"proposalHash"`
	XpubFingerprint     string       `json:"xpubFingerprint,omitempty"`
	TxIDs               []string     `json:"txIds"`
	InputOutpoints      []inputInfo  `json:"inputOutpoints"`
	Outputs             []outputInfo `json:"outputs"`
	FeeSompi            uint64       `json:"feeSompi"`
	SignaturesRequired  uint32       `json:"signaturesRequired"`
	SignaturesCollected uint32       `json:"signaturesCollected"`
	Ready               bool         `json:"ready"`
}

type mergeResponse struct {
	inspectResponse
	MergedBundleHex string          `json:"mergedBundleHex"`
	AddedSignatures []signatureInfo `json:"addedSignatures"`
}

type broadcastResponse struct {
	TxIDs []string `json:"txIds"`
}

type inputInfo struct {
	Tx                  int      `json:"tx"`
	Input               int      `json:"input"`
	TxID                string   `json:"txId"`
	Index               uint32   `json:"index"`
	AmountSompi         uint64   `json:"amountSompi"`
	DerivationPath      string   `json:"derivationPath"`
	MinimumSignatures   uint32   `json:"minimumSignatures"`
	SignaturesCollected uint32   `json:"signaturesCollected"`
	PubKeys             []string `json:"pubKeys"`
}

type outputInfo struct {
	Tx                     int    `json:"tx"`
	Output                 int    `json:"output"`
	Address                string `json:"address,omitempty"`
	AmountSompi            uint64 `json:"amountSompi"`
	ScriptPublicKey        string `json:"scriptPublicKey"`
	ScriptPublicKeyVersion uint16 `json:"scriptPublicKeyVersion"`
}

type signatureInfo struct {
	Tx        int    `json:"tx"`
	Input     int    `json:"input"`
	Slot      int    `json:"slot"`
	PubKey    string `json:"pubKey"`
	Signature string `json:"signature"`
}

func main() {
	if len(os.Args) != 2 {
		exitWithError(errors.New("usage: kaspa-pst inspect|merge|broadcast"))
	}

	var result any
	var err error
	switch os.Args[1] {
	case "inspect":
		result, err = runInspect(os.Stdin)
	case "merge":
		result, err = runMerge(os.Stdin)
	case "broadcast":
		result, err = runBroadcast(os.Stdin)
	default:
		err = errors.Errorf("unknown command %q", os.Args[1])
	}
	if err != nil {
		exitWithError(err)
	}

	encoder := json.NewEncoder(os.Stdout)
	if err := encoder.Encode(result); err != nil {
		exitWithError(err)
	}
}

func exitWithError(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}

func runInspect(reader io.Reader) (*inspectResponse, error) {
	var request inspectRequest
	if err := decodeRequest(reader, &request); err != nil {
		return nil, err
	}
	return inspectBundle(request.BundleHex, request.Network, request.FederationXpubs, request.Threshold, request.ECDSA)
}

func runMerge(reader io.Reader) (*mergeResponse, error) {
	var request mergeRequest
	if err := decodeRequest(reader, &request); err != nil {
		return nil, err
	}

	currentTransactions, err := decodeTransactionsFromHex(request.CurrentBundleHex)
	if err != nil {
		return nil, err
	}
	signedTransactions, err := decodeTransactionsFromHex(request.SignedBundleHex)
	if err != nil {
		return nil, err
	}
	if len(currentTransactions) != len(signedTransactions) {
		return nil, errors.Errorf("transaction bundle length changed")
	}

	addedSignatures := make([]signatureInfo, 0)
	mergedTransactions := make([][]byte, len(currentTransactions))
	for txIndex := range currentTransactions {
		currentPST, err := serialization.DeserializePartiallySignedTransaction(currentTransactions[txIndex])
		if err != nil {
			return nil, err
		}
		signedPST, err := serialization.DeserializePartiallySignedTransaction(signedTransactions[txIndex])
		if err != nil {
			return nil, err
		}
		added, err := mergePartiallySignedTransaction(txIndex, currentPST, signedPST)
		if err != nil {
			return nil, err
		}
		addedSignatures = append(addedSignatures, added...)
		mergedTransactions[txIndex], err = serialization.SerializePartiallySignedTransaction(currentPST)
		if err != nil {
			return nil, err
		}
	}

	if len(addedSignatures) == 0 {
		return nil, errors.New("signed bundle did not add any new signatures")
	}

	mergedBundleHex := encodeTransactionsToHex(mergedTransactions)
	inspection, err := inspectBundle(mergedBundleHex, request.Network, request.FederationXpubs, request.Threshold, request.ECDSA)
	if err != nil {
		return nil, err
	}
	return &mergeResponse{
		inspectResponse: *inspection,
		MergedBundleHex: mergedBundleHex,
		AddedSignatures: addedSignatures,
	}, nil
}

func runBroadcast(reader io.Reader) (*broadcastResponse, error) {
	var request broadcastRequest
	if err := decodeRequest(reader, &request); err != nil {
		return nil, err
	}
	if request.RPCURL == "" {
		return nil, errors.New("rpcUrl is required")
	}

	transactions, err := decodeTransactionsFromHex(request.BundleHex)
	if err != nil {
		return nil, err
	}
	if _, err := inspectBundle(request.BundleHex, request.Network, request.FederationXpubs, request.Threshold, request.ECDSA); err != nil {
		return nil, err
	}

	client, err := rpcclient.NewRPCClient(request.RPCURL)
	if err != nil {
		return nil, err
	}
	defer client.Close()

	txIDs := make([]string, len(transactions))
	for i, transaction := range transactions {
		tx, err := libkaspawallet.ExtractTransaction(transaction, request.ECDSA)
		if err != nil {
			return nil, err
		}
		txID := consensushashing.TransactionID(tx).String()
		response, err := client.SubmitTransaction(appmessage.DomainTransactionToRPCTransaction(tx), txID, false)
		if err != nil {
			return nil, err
		}
		txIDs[i] = response.TransactionID
	}
	return &broadcastResponse{TxIDs: txIDs}, nil
}

func decodeRequest(reader io.Reader, value any) error {
	decoder := json.NewDecoder(reader)
	decoder.DisallowUnknownFields()
	return decoder.Decode(value)
}

func inspectBundle(bundleHex, network string, federationXpubs []string, threshold uint32, ecdsa bool) (*inspectResponse, error) {
	params, err := paramsFromNetwork(network)
	if err != nil {
		return nil, err
	}
	transactions, err := decodeTransactionsFromHex(bundleHex)
	if err != nil {
		return nil, err
	}
	if len(transactions) == 0 {
		return nil, errors.New("empty transaction bundle")
	}

	sortedXpubs := sortedStrings(federationXpubs)
	xpubFingerprint := ""
	if len(sortedXpubs) > 0 {
		xpubFingerprint = fingerprintXpubs(sortedXpubs)
	}

	response := &inspectResponse{
		ProposalHash:    canonicalBundleHash(transactions),
		XpubFingerprint: xpubFingerprint,
		TxIDs:           make([]string, 0, len(transactions)),
		InputOutpoints:  []inputInfo{},
		Outputs:         []outputInfo{},
	}
	for txIndex, transaction := range transactions {
		pst, err := serialization.DeserializePartiallySignedTransaction(transaction)
		if err != nil {
			return nil, err
		}
		if err := verifyPartiallySignedTransaction(pst, sortedXpubs, threshold, ecdsa); err != nil {
			return nil, err
		}
		response.TxIDs = append(response.TxIDs, consensushashing.TransactionID(pst.Tx).String())

		inputAmount := uint64(0)
		outputAmount := uint64(0)
		for inputIndex, input := range pst.PartiallySignedInputs {
			collected := countSignatures(input)
			if input.MinimumSignatures > response.SignaturesRequired {
				response.SignaturesRequired = input.MinimumSignatures
			}
			if collected > response.SignaturesCollected {
				response.SignaturesCollected = collected
			}
			inputAmount += input.PrevOutput.Value
			pubKeys := make([]string, len(input.PubKeySignaturePairs))
			for i, pair := range input.PubKeySignaturePairs {
				pubKeys[i] = pair.ExtendedPublicKey
			}
			outpoint := pst.Tx.Inputs[inputIndex].PreviousOutpoint
			response.InputOutpoints = append(response.InputOutpoints, inputInfo{
				Tx:                  txIndex,
				Input:               inputIndex,
				TxID:                outpoint.TransactionID.String(),
				Index:               outpoint.Index,
				AmountSompi:         input.PrevOutput.Value,
				DerivationPath:      input.DerivationPath,
				MinimumSignatures:   input.MinimumSignatures,
				SignaturesCollected: collected,
				PubKeys:             pubKeys,
			})
		}
		for outputIndex, output := range pst.Tx.Outputs {
			outputAmount += output.Value
			info := outputInfo{
				Tx:                     txIndex,
				Output:                 outputIndex,
				AmountSompi:            output.Value,
				ScriptPublicKey:        hex.EncodeToString(output.ScriptPublicKey.Script),
				ScriptPublicKeyVersion: output.ScriptPublicKey.Version,
			}
			_, address, err := txscript.ExtractScriptPubKeyAddress(output.ScriptPublicKey, params)
			if err == nil {
				info.Address = address.String()
			}
			response.Outputs = append(response.Outputs, info)
		}
		if inputAmount < outputAmount {
			return nil, errors.Errorf("transaction %d spends more than its inputs", txIndex)
		}
		response.FeeSompi += inputAmount - outputAmount
	}
	response.Ready = isBundleReady(transactions, ecdsa)
	if response.Ready {
		for _, transaction := range transactions {
			if _, err := libkaspawallet.ExtractTransaction(transaction, ecdsa); err != nil {
				return nil, err
			}
		}
	}
	return response, nil
}

func paramsFromNetwork(network string) (*dagconfig.Params, error) {
	switch strings.ToLower(network) {
	case "", "mainnet", dagconfig.MainnetParams.Name:
		return &dagconfig.MainnetParams, nil
	case "testnet", dagconfig.TestnetParams.Name:
		return &dagconfig.TestnetParams, nil
	case "devnet", dagconfig.DevnetParams.Name:
		return &dagconfig.DevnetParams, nil
	case "simnet", dagconfig.SimnetParams.Name:
		return &dagconfig.SimnetParams, nil
	default:
		return nil, errors.Errorf("unknown network %q", network)
	}
}

func verifyPartiallySignedTransaction(pst *serialization.PartiallySignedTransaction, sortedXpubs []string, threshold uint32, ecdsa bool) error {
	if len(pst.Tx.Inputs) != len(pst.PartiallySignedInputs) {
		return errors.New("transaction input count does not match partially signed input count")
	}
	for inputIndex, input := range pst.PartiallySignedInputs {
		if input.PrevOutput == nil {
			return errors.Errorf("input %d is missing previous output", inputIndex)
		}
		if input.MinimumSignatures == 0 {
			return errors.Errorf("input %d has zero minimum signatures", inputIndex)
		}
		if threshold != 0 && input.MinimumSignatures != threshold {
			return errors.Errorf("input %d threshold mismatch", inputIndex)
		}
		if uint32(len(input.PubKeySignaturePairs)) < input.MinimumSignatures {
			return errors.Errorf("input %d has fewer pubkey slots than required signatures", inputIndex)
		}
		if len(sortedXpubs) > 0 {
			if len(input.PubKeySignaturePairs) != len(sortedXpubs) {
				return errors.Errorf("input %d pubkey slot count does not match federation xpub count", inputIndex)
			}
			expected, err := deriveXpubs(sortedXpubs, input.DerivationPath)
			if err != nil {
				return err
			}
			for slot, expectedXpub := range expected {
				if input.PubKeySignaturePairs[slot].ExtendedPublicKey != expectedXpub {
					return errors.Errorf("input %d pubkey slot %d does not match federation derivation", inputIndex, slot)
				}
			}
		}
	}
	return nil
}

func mergePartiallySignedTransaction(txIndex int, current, signed *serialization.PartiallySignedTransaction) ([]signatureInfo, error) {
	if err := ensureSameUnsignedTransaction(current.Tx, signed.Tx); err != nil {
		return nil, errors.Wrapf(err, "transaction %d changed", txIndex)
	}
	if len(current.PartiallySignedInputs) != len(signed.PartiallySignedInputs) {
		return nil, errors.Errorf("transaction %d partially signed input count changed", txIndex)
	}

	added := make([]signatureInfo, 0)
	for inputIndex := range current.PartiallySignedInputs {
		currentInput := current.PartiallySignedInputs[inputIndex]
		signedInput := signed.PartiallySignedInputs[inputIndex]
		if err := ensureSamePartiallySignedInput(currentInput, signedInput); err != nil {
			return nil, errors.Wrapf(err, "transaction %d input %d changed", txIndex, inputIndex)
		}
		for slot, signedPair := range signedInput.PubKeySignaturePairs {
			currentPair := currentInput.PubKeySignaturePairs[slot]
			if len(signedPair.Signature) == 0 {
				continue
			}
			if len(currentPair.Signature) != 0 {
				if !bytes.Equal(currentPair.Signature, signedPair.Signature) {
					return nil, errors.Errorf("transaction %d input %d slot %d has conflicting signature", txIndex, inputIndex, slot)
				}
				continue
			}
			currentPair.Signature = append([]byte(nil), signedPair.Signature...)
			added = append(added, signatureInfo{
				Tx:        txIndex,
				Input:     inputIndex,
				Slot:      slot,
				PubKey:    currentPair.ExtendedPublicKey,
				Signature: hex.EncodeToString(currentPair.Signature),
			})
		}
	}
	return added, nil
}

func ensureSameUnsignedTransaction(current, signed *externalapi.DomainTransaction) error {
	if current.Version != signed.Version || current.LockTime != signed.LockTime || current.Gas != signed.Gas ||
		!current.SubnetworkID.Equal(&signed.SubnetworkID) || !bytes.Equal(current.Payload, signed.Payload) {
		return errors.New("transaction metadata changed")
	}
	if len(current.Inputs) != len(signed.Inputs) {
		return errors.New("input count changed")
	}
	if len(current.Outputs) != len(signed.Outputs) {
		return errors.New("output count changed")
	}
	for i := range current.Inputs {
		if !current.Inputs[i].PreviousOutpoint.Equal(&signed.Inputs[i].PreviousOutpoint) {
			return errors.Errorf("input %d outpoint changed", i)
		}
		if current.Inputs[i].Sequence != signed.Inputs[i].Sequence {
			return errors.Errorf("input %d sequence changed", i)
		}
	}
	for i := range current.Outputs {
		if !current.Outputs[i].Equal(signed.Outputs[i]) {
			return errors.Errorf("output %d changed", i)
		}
	}
	return nil
}

func ensureSamePartiallySignedInput(current, signed *serialization.PartiallySignedInput) error {
	if current.PrevOutput == nil || signed.PrevOutput == nil || !current.PrevOutput.Equal(signed.PrevOutput) {
		return errors.New("previous output changed")
	}
	if current.MinimumSignatures != signed.MinimumSignatures {
		return errors.New("minimum signatures changed")
	}
	if current.DerivationPath != signed.DerivationPath {
		return errors.New("derivation path changed")
	}
	if len(current.PubKeySignaturePairs) != len(signed.PubKeySignaturePairs) {
		return errors.New("pubkey slot count changed")
	}
	for i := range current.PubKeySignaturePairs {
		if current.PubKeySignaturePairs[i].ExtendedPublicKey != signed.PubKeySignaturePairs[i].ExtendedPublicKey {
			return errors.Errorf("pubkey slot %d changed", i)
		}
	}
	return nil
}

func isBundleReady(transactions [][]byte, ecdsa bool) bool {
	for _, transaction := range transactions {
		fullySigned, err := libkaspawallet.IsTransactionFullySigned(transaction)
		if err != nil || !fullySigned {
			return false
		}
	}
	return true
}

func countSignatures(input *serialization.PartiallySignedInput) uint32 {
	count := uint32(0)
	for _, pair := range input.PubKeySignaturePairs {
		if len(pair.Signature) != 0 {
			count++
		}
	}
	return count
}

func deriveXpubs(xpubs []string, path string) ([]string, error) {
	derived := make([]string, len(xpubs))
	for i, xpub := range xpubs {
		extendedKey, err := bip32.DeserializeExtendedKey(xpub)
		if err != nil {
			return nil, err
		}
		derivedKey, err := extendedKey.DeriveFromPath(path)
		if err != nil {
			return nil, err
		}
		derived[i] = derivedKey.String()
	}
	return derived, nil
}

func decodeTransactionsFromHex(transactionsHex string) ([][]byte, error) {
	transactionsHex = strings.TrimSpace(transactionsHex)
	if transactionsHex == "" {
		return nil, errors.New("empty transaction bundle")
	}
	splitTransactionsHexes := strings.Split(transactionsHex, "_")
	transactions := make([][]byte, len(splitTransactionsHexes))
	for i, transactionHex := range splitTransactionsHexes {
		transaction, err := hex.DecodeString(transactionHex)
		if err != nil {
			return nil, err
		}
		transactions[i] = transaction
	}
	return transactions, nil
}

func encodeTransactionsToHex(transactions [][]byte) string {
	transactionsInHex := make([]string, len(transactions))
	for i, transaction := range transactions {
		transactionsInHex[i] = hex.EncodeToString(transaction)
	}
	return strings.Join(transactionsInHex, "_")
}

func canonicalBundleHash(transactions [][]byte) string {
	hasher := sha256.New()
	for _, transaction := range transactions {
		var length [8]byte
		binary.LittleEndian.PutUint64(length[:], uint64(len(transaction)))
		hasher.Write(length[:])
		hasher.Write(transaction)
	}
	return hex.EncodeToString(hasher.Sum(nil))
}

func fingerprintXpubs(xpubs []string) string {
	data, err := json.Marshal(xpubs)
	if err != nil {
		panic(err)
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func sortedStrings(values []string) []string {
	result := make([]string, 0, len(values))
	for _, value := range values {
		if value != "" {
			result = append(result, strings.TrimSpace(value))
		}
	}
	sort.Strings(result)
	return result
}
