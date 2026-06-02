package main

import (
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"sort"

	"github.com/kaspanet/kaspad/cmd/kaspawallet/keys"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet"
	"github.com/kaspanet/kaspad/domain/dagconfig"
	"github.com/kaspanet/kaspad/infrastructure/network/rpcclient"
	"github.com/kaspanet/kaspad/util"
	"github.com/pkg/errors"
)

type signerMetadata struct {
	Index         int    `json:"index"`
	CosignerIndex uint32 `json:"cosignerIndex"`
	Xpub          string `json:"xpub"`
	KeysFile      string `json:"keysFile"`
	DaemonPort    int    `json:"daemonPort"`
}

type keyMetadata struct {
	PrivateKeyHex string `json:"privateKeyHex"`
	MiningAddress string `json:"miningAddress,omitempty"`
	DevnetAddress string `json:"devnetAddress,omitempty"`
	Address       string `json:"address,omitempty"`
}

type metadata struct {
	Network          string           `json:"network"`
	Threshold        uint32           `json:"threshold"`
	Xpubs            []string         `json:"xpubs"`
	Signers          []signerMetadata `json:"signers"`
	CanonicalPath    string           `json:"canonicalPath"`
	CanonicalAddress string           `json:"canonicalAddress"`
	Faucet           keyMetadata      `json:"faucet"`
	Recipient        keyMetadata      `json:"recipient"`
}

func main() {
	if len(os.Args) < 2 {
		fatalf("usage: kaspa-msig-fixture <create|balance|utxos|dag-info> [flags]")
	}

	var err error
	switch os.Args[1] {
	case "create":
		err = create(os.Args[2:])
	case "balance":
		err = balance(os.Args[2:])
	case "utxos":
		err = utxos(os.Args[2:])
	case "dag-info":
		err = dagInfo(os.Args[2:])
	default:
		err = errors.Errorf("unknown command %q", os.Args[1])
	}
	if err != nil {
		fatalf("%+v", err)
	}
}

func create(args []string) error {
	fs := flag.NewFlagSet("create", flag.ExitOnError)
	network := fs.String("network", "devnet", "network: devnet or testnet")
	outDir := fs.String("out-dir", "/work/wallets", "output directory")
	password := fs.String("password", "stage-msig-pass", "wallet password")
	threshold := fs.Uint("threshold", 2, "minimum signatures")
	signers := fs.Int("signers", 3, "number of signers")
	force := fs.Bool("force", false, "overwrite existing metadata and keys")
	if err := fs.Parse(args); err != nil {
		return err
	}

	params, err := paramsFor(*network)
	if err != nil {
		return err
	}
	if *signers < 2 {
		return errors.Errorf("signers must be at least 2")
	}
	if *threshold == 0 || int(*threshold) > *signers {
		return errors.Errorf("threshold must be between 1 and signer count")
	}

	if err := os.MkdirAll(*outDir, 0o700); err != nil {
		return err
	}
	metadataPath := filepath.Join(*outDir, "metadata.json")
	if _, err := os.Stat(metadataPath); err == nil && !*force {
		return errors.Errorf("%s already exists; pass --force to overwrite", metadataPath)
	}

	encryptedBySigner := make([]*keys.EncryptedMnemonic, *signers)
	xpubBySigner := make([]string, *signers)
	for i := 0; i < *signers; i++ {
		encryptedMnemonics, xpubs, err := keys.CreateMnemonics(params, 1, *password, true)
		if err != nil {
			return err
		}
		encryptedBySigner[i] = encryptedMnemonics[0]
		xpubBySigner[i] = xpubs[0]
	}

	sortedXpubs := append([]string(nil), xpubBySigner...)
	sort.Strings(sortedXpubs)

	signersMetadata := make([]signerMetadata, *signers)
	for i := 0; i < *signers; i++ {
		cosignerIndex, err := cosignerIndex(xpubBySigner[i], sortedXpubs)
		if err != nil {
			return err
		}
		keysPath := filepath.Join(*outDir, fmt.Sprintf("signer-%d.keys.json", i))
		file := keys.File{
			Version:            keys.LastVersion,
			EncryptedMnemonics: []*keys.EncryptedMnemonic{encryptedBySigner[i]},
			ExtendedPublicKeys: sortedXpubs,
			MinimumSignatures:  uint32(*threshold),
			CosignerIndex:      cosignerIndex,
			ECDSA:              false,
		}
		if err := file.SetPath(params, keysPath, true); err != nil {
			return err
		}
		if err := file.Save(); err != nil {
			return err
		}
		signersMetadata[i] = signerMetadata{
			Index:         i,
			CosignerIndex: cosignerIndex,
			Xpub:          xpubBySigner[i],
			KeysFile:      keysPath,
			DaemonPort:    8082 + i,
		}
	}

	canonicalPath := "m/0/0/1"
	canonicalAddress, err := multisigAddress(params, sortedXpubs, uint32(*threshold), canonicalPath)
	if err != nil {
		return err
	}

	faucet, err := faucetKey(params)
	if err != nil {
		return err
	}
	recipient, err := recipientKey(params)
	if err != nil {
		return err
	}

	metadata := metadata{
		Network:          *network,
		Threshold:        uint32(*threshold),
		Xpubs:            sortedXpubs,
		Signers:          signersMetadata,
		CanonicalPath:    canonicalPath,
		CanonicalAddress: canonicalAddress,
		Faucet:           faucet,
		Recipient:        recipient,
	}

	file, err := os.OpenFile(metadataPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o600)
	if err != nil {
		return err
	}
	defer file.Close()
	encoder := json.NewEncoder(file)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(metadata); err != nil {
		return err
	}

	return json.NewEncoder(os.Stdout).Encode(metadata)
}

func balance(args []string) error {
	fs := flag.NewFlagSet("balance", flag.ExitOnError)
	rpc := fs.String("rpc", "kaspad:16610", "RPC address")
	address := fs.String("address", "", "address")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *address == "" {
		return errors.Errorf("--address is required")
	}
	client, err := rpcclient.NewRPCClient(*rpc)
	if err != nil {
		return err
	}
	defer client.Close()
	response, err := client.GetBalancesByAddresses([]string{*address})
	if err != nil {
		return err
	}
	var balance uint64
	for _, entry := range response.Entries {
		if entry.Address == *address {
			balance = entry.Balance
			break
		}
	}
	return json.NewEncoder(os.Stdout).Encode(map[string]any{
		"address":      *address,
		"balanceSompi": balance,
	})
}

func utxos(args []string) error {
	fs := flag.NewFlagSet("utxos", flag.ExitOnError)
	rpc := fs.String("rpc", "kaspad:16610", "RPC address")
	address := fs.String("address", "", "address")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *address == "" {
		return errors.Errorf("--address is required")
	}
	client, err := rpcclient.NewRPCClient(*rpc)
	if err != nil {
		return err
	}
	defer client.Close()
	response, err := client.GetUTXOsByAddresses([]string{*address})
	if err != nil {
		return err
	}
	entries := make([]map[string]any, 0, len(response.Entries))
	for _, entry := range response.Entries {
		if entry.Address != *address || entry.Outpoint == nil || entry.UTXOEntry == nil || entry.UTXOEntry.ScriptPublicKey == nil {
			continue
		}
		entries = append(entries, map[string]any{
			"address": entry.Address,
			"outpoint": map[string]any{
				"transactionId": entry.Outpoint.TransactionID,
				"index":         entry.Outpoint.Index,
			},
			"utxoEntry": map[string]any{
				"amount":        entry.UTXOEntry.Amount,
				"blockDaaScore": entry.UTXOEntry.BlockDAAScore,
				"isCoinbase":    entry.UTXOEntry.IsCoinbase,
				"scriptPublicKey": map[string]any{
					"version": entry.UTXOEntry.ScriptPublicKey.Version,
					"script":  entry.UTXOEntry.ScriptPublicKey.Script,
				},
			},
		})
	}
	return json.NewEncoder(os.Stdout).Encode(map[string]any{
		"address": *address,
		"entries": entries,
	})
}

func dagInfo(args []string) error {
	fs := flag.NewFlagSet("dag-info", flag.ExitOnError)
	rpc := fs.String("rpc", "kaspad:16610", "RPC address")
	if err := fs.Parse(args); err != nil {
		return err
	}
	client, err := rpcclient.NewRPCClient(*rpc)
	if err != nil {
		return err
	}
	defer client.Close()
	info, err := client.GetBlockDAGInfo()
	if err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(map[string]any{
		"networkName":     info.NetworkName,
		"blockCount":      info.BlockCount,
		"headerCount":     info.HeaderCount,
		"virtualDaaScore": info.VirtualDAAScore,
	})
}

func paramsFor(network string) (*dagconfig.Params, error) {
	switch network {
	case "devnet":
		return &dagconfig.DevnetParams, nil
	case "testnet":
		return &dagconfig.TestnetParams, nil
	default:
		return nil, errors.Errorf("unsupported network %q", network)
	}
}

func cosignerIndex(xpub string, sortedXpubs []string) (uint32, error) {
	index := sort.SearchStrings(sortedXpubs, xpub)
	if index == len(sortedXpubs) || sortedXpubs[index] != xpub {
		return 0, errors.Errorf("could not find xpub in sorted xpub set")
	}
	return uint32(index), nil
}

func multisigAddress(params *dagconfig.Params, xpubs []string, threshold uint32, path string) (string, error) {
	xpubCopy := append([]string(nil), xpubs...)
	address, err := libkaspawallet.Address(params, xpubCopy, threshold, path, false)
	if err != nil {
		return "", err
	}
	return address.String(), nil
}

func faucetKey(params *dagconfig.Params) (keyMetadata, error) {
	privateKey, publicKey, err := libkaspawallet.CreateKeyPair(false)
	if err != nil {
		return keyMetadata{}, err
	}
	devnetAddress, err := util.NewAddressPublicKey(publicKey, params.Prefix)
	if err != nil {
		return keyMetadata{}, err
	}
	return keyMetadata{
		PrivateKeyHex: hex.EncodeToString(privateKey),
		MiningAddress: devnetAddress.String(),
		DevnetAddress: devnetAddress.String(),
	}, nil
}

func recipientKey(params *dagconfig.Params) (keyMetadata, error) {
	privateKey, publicKey, err := libkaspawallet.CreateKeyPair(false)
	if err != nil {
		return keyMetadata{}, err
	}
	address, err := util.NewAddressPublicKey(publicKey, params.Prefix)
	if err != nil {
		return keyMetadata{}, err
	}
	return keyMetadata{
		PrivateKeyHex: hex.EncodeToString(privateKey),
		Address:       address.String(),
	}, nil
}

func fatalf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
