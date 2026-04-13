package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/kaspanet/kaspad/cmd/kaspawallet/daemon/client"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/daemon/pb"
	walletserver "github.com/kaspanet/kaspad/cmd/kaspawallet/daemon/server"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/keys"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet/bip32"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet/serialization"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/utils"
	"github.com/kaspanet/kaspad/domain/consensus/utils/consensushashing"
	"github.com/kaspanet/kaspad/domain/consensus/utils/txscript"
	"github.com/kaspanet/kaspad/domain/dagconfig"
	"github.com/kaspanet/kaspad/util/txmass"
	"github.com/tyler-smith/go-bip39"
)

const (
	defaultDaemonRPCServer = "localhost"
	defaultDaemonTimeout   = 30
)

type backendApp struct {
	mu     sync.RWMutex
	daemon *daemonHandle
}

type daemonHandle struct {
	Network   string
	KeysFile  string
	RPCServer string
	Address   string
	StartedAt time.Time

	FinishedAt *time.Time
	ExitError  string
}

type apiError struct {
	Error string `json:"error"`
}

type requestHandler[T any] func(context.Context, T) (any, error)

type walletSummaryRequest struct {
	Network  string `json:"network"`
	KeysFile string `json:"keysFile"`
}

type walletSummaryResponse struct {
	Network                 string   `json:"network"`
	KeysFile                string   `json:"keysFile"`
	PublicKeys              []string `json:"publicKeys"`
	SortedPublicKeys        []string `json:"sortedPublicKeys"`
	PublicKeyCount          int      `json:"publicKeyCount"`
	OwnedKeyCount           int      `json:"ownedKeyCount"`
	MinimumSignatures       uint32   `json:"minimumSignatures"`
	CosignerIndex           uint32   `json:"cosignerIndex"`
	LastUsedExternalIndex   uint32   `json:"lastUsedExternalIndex"`
	LastUsedInternalIndex   uint32   `json:"lastUsedInternalIndex"`
	Fingerprint             string   `json:"fingerprint"`
	IsMultisig              bool     `json:"isMultisig"`
	HasPrivateKeys          bool     `json:"hasPrivateKeys"`
	OwnsAllKeys             bool     `json:"ownsAllKeys"`
	IsCanonicalAddressOwner bool     `json:"isCanonicalAddressOwner"`
	ECDSA                   bool     `json:"ecdsa"`
}

type bootstrapCreateRequest struct {
	Network           string   `json:"network"`
	KeysFile          string   `json:"keysFile"`
	Password          string   `json:"password"`
	MinimumSignatures uint32   `json:"minimumSignatures"`
	NumPrivateKeys    uint32   `json:"numPrivateKeys"`
	NumPublicKeys     uint32   `json:"numPublicKeys"`
	RemotePublicKeys  []string `json:"remotePublicKeys"`
	ImportMnemonics   []string `json:"importMnemonics"`
	ECDSA             bool     `json:"ecdsa"`
	Overwrite         bool     `json:"overwrite"`
}

type bootstrapCreateResponse struct {
	Summary               walletSummaryResponse `json:"summary"`
	LocalExtendedPubKeys  []string              `json:"localExtendedPubKeys"`
	LocalMnemonics        []string              `json:"localMnemonics"`
	CanonicalOwnerWarning string                `json:"canonicalOwnerWarning"`
}

type exportSecretsRequest struct {
	Network  string `json:"network"`
	KeysFile string `json:"keysFile"`
	Password string `json:"password"`
}

type exportSecretsResponse struct {
	Mnemonics          []string `json:"mnemonics"`
	ExternalPublicKeys []string `json:"externalPublicKeys"`
	MinimumSignatures  uint32   `json:"minimumSignatures"`
}

type daemonStartRequest struct {
	Network        string `json:"network"`
	KeysFile       string `json:"keysFile"`
	RPCServer      string `json:"rpcServer"`
	Listen         string `json:"listen"`
	TimeoutSeconds uint32 `json:"timeoutSeconds"`
}

type daemonStatusResponse struct {
	State         string                 `json:"state"`
	Message       string                 `json:"message"`
	DaemonAddress string                 `json:"daemonAddress"`
	Network       string                 `json:"network"`
	KeysFile      string                 `json:"keysFile"`
	RPCServer     string                 `json:"rpcServer"`
	StartedAt     string                 `json:"startedAt,omitempty"`
	FinishedAt    string                 `json:"finishedAt,omitempty"`
	WalletVersion string                 `json:"walletVersion,omitempty"`
	Wallet        *walletSummaryResponse `json:"wallet,omitempty"`
}

type balanceResponse struct {
	AvailableSompi uint64               `json:"availableSompi"`
	PendingSompi   uint64               `json:"pendingSompi"`
	AvailableKas   string               `json:"availableKas"`
	PendingKas     string               `json:"pendingKas"`
	Addresses      []addressBalanceView `json:"addresses"`
}

type addressBalanceView struct {
	Address        string `json:"address"`
	AvailableSompi uint64 `json:"availableSompi"`
	PendingSompi   uint64 `json:"pendingSompi"`
	AvailableKas   string `json:"availableKas"`
	PendingKas     string `json:"pendingKas"`
}

type addressesResponse struct {
	Addresses []string `json:"addresses"`
}

type newAddressResponse struct {
	Address string `json:"address"`
}

type feePolicyRequest struct {
	ExactFeeRate *float64 `json:"exactFeeRate"`
	MaxFeeRate   *float64 `json:"maxFeeRate"`
	MaxFee       *uint64  `json:"maxFee"`
}

type createUnsignedRequest struct {
	ToAddress                string           `json:"toAddress"`
	AmountKas                string           `json:"amountKas"`
	SendAll                  bool             `json:"sendAll"`
	FromAddresses            []string         `json:"fromAddresses"`
	UseExistingChangeAddress bool             `json:"useExistingChangeAddress"`
	FeePolicy                feePolicyRequest `json:"feePolicy"`
}

type signRequest struct {
	Network         string `json:"network"`
	KeysFile        string `json:"keysFile"`
	Password        string `json:"password"`
	TransactionsHex string `json:"transactionsHex"`
}

type broadcastRequest struct {
	TransactionsHex string `json:"transactionsHex"`
}

type parseRequest struct {
	Network         string `json:"network"`
	KeysFile        string `json:"keysFile"`
	TransactionsHex string `json:"transactionsHex"`
}

type transactionBundleResponse struct {
	TransactionsHex  string                  `json:"transactionsHex"`
	TransactionCount int                     `json:"transactionCount"`
	FullySigned      bool                    `json:"fullySigned"`
	Transactions     []parsedTransactionView `json:"transactions"`
}

type parsedTransactionView struct {
	Index           int                          `json:"index"`
	TxID            string                       `json:"txId"`
	FullySigned     bool                         `json:"fullySigned"`
	InputCount      int                          `json:"inputCount"`
	OutputCount     int                          `json:"outputCount"`
	Inputs          []parsedInputView            `json:"inputs"`
	Outputs         []parsedOutputView           `json:"outputs"`
	FeeSompi        uint64                       `json:"feeSompi"`
	FeeKas          string                       `json:"feeKas"`
	Mass            uint64                       `json:"mass"`
	FeeRate         float64                      `json:"feeRate"`
	HasMassEstimate bool                         `json:"hasMassEstimate"`
	Signatures      []inputSignatureProgressView `json:"signatures"`
}

type parsedInputView struct {
	Outpoint    string `json:"outpoint"`
	AmountSompi uint64 `json:"amountSompi"`
	AmountKas   string `json:"amountKas"`
}

type parsedOutputView struct {
	Address     string `json:"address"`
	AmountSompi uint64 `json:"amountSompi"`
	AmountKas   string `json:"amountKas"`
}

type inputSignatureProgressView struct {
	InputIndex        int    `json:"inputIndex"`
	SignedBy          int    `json:"signedBy"`
	MinimumSignatures uint32 `json:"minimumSignatures"`
	MissingSignatures uint32 `json:"missingSignatures"`
}

func main() {
	listen := flag.String("listen", "127.0.0.1:0", "HTTP listen address for the local GUI bridge")
	flag.Parse()

	app := &backendApp{}
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
	})
	mux.HandleFunc("/api/wallet/summary", wrap(app.handleWalletSummary))
	mux.HandleFunc("/api/bootstrap/create", wrap(app.handleBootstrapCreate))
	mux.HandleFunc("/api/bootstrap/export-secrets", wrap(app.handleExportSecrets))
	mux.HandleFunc("/api/daemon/start", wrap(app.handleDaemonStart))
	mux.HandleFunc("/api/daemon/stop", wrap(app.handleDaemonStop))
	mux.HandleFunc("/api/daemon/status", wrap(app.handleDaemonStatus))
	mux.HandleFunc("/api/balance", wrap(app.handleBalance))
	mux.HandleFunc("/api/addresses/list", wrap(app.handleListAddresses))
	mux.HandleFunc("/api/addresses/new", wrap(app.handleNewAddress))
	mux.HandleFunc("/api/transactions/create-unsigned", wrap(app.handleCreateUnsigned))
	mux.HandleFunc("/api/transactions/sign", wrap(app.handleSign))
	mux.HandleFunc("/api/transactions/broadcast", wrap(app.handleBroadcast))
	mux.HandleFunc("/api/transactions/parse", wrap(app.handleParse))

	listener, err := net.Listen("tcp", *listen)
	if err != nil {
		fmt.Fprintf(os.Stderr, "failed listening on %s: %s\n", *listen, err)
		os.Exit(1)
	}

	httpServer := &http.Server{
		Handler:           mux,
		ReadHeaderTimeout: 5 * time.Second,
		IdleTimeout:       30 * time.Second,
	}

	fmt.Printf("READY http://%s\n", listener.Addr().String())
	if err := httpServer.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
		fmt.Fprintf(os.Stderr, "backend server stopped: %s\n", err)
		os.Exit(1)
	}
}

func wrap[T any](handler requestHandler[T]) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, apiError{Error: "POST required"})
			return
		}

		var request T
		decoder := json.NewDecoder(r.Body)
		decoder.DisallowUnknownFields()
		if err := decoder.Decode(&request); err != nil {
			writeJSON(w, http.StatusBadRequest, apiError{Error: err.Error()})
			return
		}

		response, err := handler(r.Context(), request)
		if err != nil {
			writeJSON(w, http.StatusBadRequest, apiError{Error: err.Error()})
			return
		}

		writeJSON(w, http.StatusOK, response)
	}
}

func writeJSON(w http.ResponseWriter, status int, payload any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(payload); err != nil {
		fmt.Fprintf(os.Stderr, "failed writing JSON response: %s\n", err)
	}
}

func (b *backendApp) handleWalletSummary(_ context.Context, request walletSummaryRequest) (any, error) {
	request.KeysFile = expandUserPath(request.KeysFile)
	params, err := paramsForNetwork(request.Network)
	if err != nil {
		return nil, err
	}

	keysFile, err := keys.ReadKeysFile(params, request.KeysFile)
	if err != nil {
		return nil, err
	}

	return walletSummaryFromKeysFile(request.Network, keysFile), nil
}

func (b *backendApp) handleBootstrapCreate(_ context.Context, request bootstrapCreateRequest) (any, error) {
	request.KeysFile = expandUserPath(request.KeysFile)
	params, err := paramsForNetwork(request.Network)
	if err != nil {
		return nil, err
	}
	if request.Password == "" {
		return nil, fmt.Errorf("password is required")
	}
	if request.NumPrivateKeys == 0 {
		request.NumPrivateKeys = 1
	}
	if request.NumPublicKeys == 0 {
		request.NumPublicKeys = 1
	}
	if request.NumPrivateKeys > request.NumPublicKeys {
		return nil, fmt.Errorf("numPrivateKeys cannot exceed numPublicKeys")
	}
	if request.MinimumSignatures == 0 {
		request.MinimumSignatures = 1
	}
	if request.MinimumSignatures > request.NumPublicKeys {
		return nil, fmt.Errorf("minimum signatures cannot exceed public key count")
	}
	if len(request.RemotePublicKeys) != int(request.NumPublicKeys-request.NumPrivateKeys) {
		return nil, fmt.Errorf("expected %d remote public keys, got %d",
			request.NumPublicKeys-request.NumPrivateKeys, len(request.RemotePublicKeys))
	}

	isMultisig := request.NumPublicKeys > 1
	for _, publicKey := range request.RemotePublicKeys {
		if _, err := bip32.DeserializeExtendedKey(publicKey); err != nil {
			return nil, fmt.Errorf("%s is not a valid extended public key: %w", publicKey, err)
		}
	}

	var (
		encryptedMnemonics   []*keys.EncryptedMnemonic
		localExtendedPubKeys []string
		localMnemonics       []string
	)

	if len(request.ImportMnemonics) > 0 {
		if len(request.ImportMnemonics) != int(request.NumPrivateKeys) {
			return nil, fmt.Errorf("expected %d mnemonics, got %d", request.NumPrivateKeys, len(request.ImportMnemonics))
		}
		for _, mnemonic := range request.ImportMnemonics {
			if !bip39.IsMnemonicValid(mnemonic) {
				return nil, fmt.Errorf("invalid mnemonic supplied")
			}
		}
		encryptedMnemonics, localExtendedPubKeys, err = keys.EncryptMnemonicsAndPublicKeys(params, request.ImportMnemonics, request.Password, isMultisig)
		localMnemonics = append(localMnemonics, request.ImportMnemonics...)
	} else {
		encryptedMnemonics, localExtendedPubKeys, err = keys.CreateMnemonics(params, request.NumPrivateKeys, request.Password, isMultisig)
		if err == nil {
			tmpFile := &keys.File{
				Version:            keys.LastVersion,
				EncryptedMnemonics: encryptedMnemonics,
				ExtendedPublicKeys: localExtendedPubKeys,
			}
			localMnemonics, err = tmpFile.DecryptMnemonics(request.Password)
		}
	}
	if err != nil {
		return nil, err
	}

	allExtendedPubKeys := append(append([]string{}, localExtendedPubKeys...), request.RemotePublicKeys...)
	if hasDuplicateStrings(allExtendedPubKeys) {
		return nil, fmt.Errorf("duplicate public keys are not allowed")
	}

	cosignerIndex, err := libkaspawallet.MinimumCosignerIndex(localExtendedPubKeys, allExtendedPubKeys)
	if err != nil {
		return nil, err
	}

	if request.KeysFile != "" {
		if exists, err := pathExists(request.KeysFile); err != nil {
			return nil, err
		} else if exists && !request.Overwrite {
			return nil, fmt.Errorf("keys file %s already exists; enable overwrite to replace it", request.KeysFile)
		}
	}

	file := &keys.File{
		Version:            keys.LastVersion,
		EncryptedMnemonics: encryptedMnemonics,
		ExtendedPublicKeys: allExtendedPubKeys,
		MinimumSignatures:  request.MinimumSignatures,
		CosignerIndex:      cosignerIndex,
		ECDSA:              request.ECDSA,
	}
	if err := file.SetPath(params, request.KeysFile, true); err != nil {
		return nil, err
	}
	if err := file.Save(); err != nil {
		return nil, err
	}

	response := bootstrapCreateResponse{
		Summary:              walletSummaryFromKeysFile(params.Name, file),
		LocalExtendedPubKeys: localExtendedPubKeys,
		LocalMnemonics:       localMnemonics,
	}
	if cosignerIndex == 0 && request.NumPublicKeys > 1 {
		response.CanonicalOwnerWarning = "This cosigner is index 0 and should be the canonical address generator for receive flow."
	}

	return response, nil
}

func (b *backendApp) handleExportSecrets(_ context.Context, request exportSecretsRequest) (any, error) {
	request.KeysFile = expandUserPath(request.KeysFile)
	params, err := paramsForNetwork(request.Network)
	if err != nil {
		return nil, err
	}
	if request.Password == "" {
		return nil, fmt.Errorf("password is required")
	}

	keysFile, err := keys.ReadKeysFile(params, request.KeysFile)
	if err != nil {
		return nil, err
	}
	mnemonics, err := keysFile.DecryptMnemonics(request.Password)
	if err != nil {
		return nil, err
	}

	mnemonicPublicKeys := make(map[string]struct{}, len(mnemonics))
	for _, mnemonic := range mnemonics {
		publicKey, err := libkaspawallet.MasterPublicKeyFromMnemonic(params, mnemonic, len(keysFile.ExtendedPublicKeys) > 1)
		if err != nil {
			return nil, err
		}
		mnemonicPublicKeys[publicKey] = struct{}{}
	}

	externalPublicKeys := make([]string, 0, len(keysFile.ExtendedPublicKeys))
	for _, publicKey := range keysFile.ExtendedPublicKeys {
		if _, ok := mnemonicPublicKeys[publicKey]; ok {
			continue
		}
		externalPublicKeys = append(externalPublicKeys, publicKey)
	}

	return exportSecretsResponse{
		Mnemonics:          mnemonics,
		ExternalPublicKeys: externalPublicKeys,
		MinimumSignatures:  keysFile.MinimumSignatures,
	}, nil
}

func (b *backendApp) handleDaemonStart(ctx context.Context, request daemonStartRequest) (any, error) {
	request.KeysFile = expandUserPath(request.KeysFile)
	params, err := paramsForNetwork(request.Network)
	if err != nil {
		return nil, err
	}
	if request.KeysFile == "" {
		return nil, fmt.Errorf("keys file is required")
	}
	if request.RPCServer == "" {
		request.RPCServer = defaultDaemonRPCServer
	}
	if request.TimeoutSeconds == 0 {
		request.TimeoutSeconds = defaultDaemonTimeout
	}
	if request.Listen == "" {
		request.Listen, err = findFreeLoopbackAddress()
		if err != nil {
			return nil, err
		}
	}

	b.mu.Lock()
	if b.daemon != nil && b.daemon.FinishedAt == nil {
		b.mu.Unlock()
		return nil, fmt.Errorf("daemon already running on %s", b.daemon.Address)
	}

	handle := &daemonHandle{
		Network:   params.Name,
		KeysFile:  request.KeysFile,
		RPCServer: request.RPCServer,
		Address:   request.Listen,
		StartedAt: time.Now(),
	}
	b.daemon = handle
	b.mu.Unlock()

	go func(expected *daemonHandle) {
		runErr := walletserver.Start(params, expected.Address, expected.RPCServer, expected.KeysFile, "", request.TimeoutSeconds)
		finishedAt := time.Now()

		b.mu.Lock()
		defer b.mu.Unlock()
		if b.daemon != expected {
			return
		}
		expected.FinishedAt = &finishedAt
		if runErr != nil {
			expected.ExitError = runErr.Error()
		}
	}(handle)

	if err := waitForDaemonReady(ctx, request.Listen, 5*time.Second); err != nil {
		return nil, err
	}

	return b.handleDaemonStatus(ctx, struct{}{})
}

func (b *backendApp) handleDaemonStop(ctx context.Context, _ struct{}) (any, error) {
	b.mu.RLock()
	handle := b.daemon
	b.mu.RUnlock()

	if handle == nil {
		return daemonStatusResponse{State: "stopped", Message: "daemon is not running"}, nil
	}

	clientConn, tearDown, err := client.Connect(handle.Address)
	if err == nil {
		shutdownCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
		_, shutdownErr := clientConn.Shutdown(shutdownCtx, &pb.ShutdownRequest{})
		cancel()
		tearDown()
		if shutdownErr != nil {
			return nil, shutdownErr
		}
	}

	deadline := time.Now().Add(5 * time.Second)
	for {
		b.mu.RLock()
		finished := handle.FinishedAt != nil
		b.mu.RUnlock()
		if finished || time.Now().After(deadline) {
			break
		}
		time.Sleep(100 * time.Millisecond)
	}

	b.mu.Lock()
	if b.daemon == handle {
		b.daemon = nil
	}
	b.mu.Unlock()

	return daemonStatusResponse{
		State:         "stopped",
		Message:       "daemon stopped",
		DaemonAddress: handle.Address,
		Network:       handle.Network,
		KeysFile:      handle.KeysFile,
		RPCServer:     handle.RPCServer,
	}, nil
}

func (b *backendApp) handleDaemonStatus(ctx context.Context, _ struct{}) (any, error) {
	return b.currentDaemonStatus(ctx)
}

func (b *backendApp) handleBalance(ctx context.Context, _ struct{}) (any, error) {
	daemonInfo, walletClient, tearDown, err := b.connectCurrentDaemon()
	if err != nil {
		return nil, err
	}
	defer tearDown()

	callCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()

	response, err := walletClient.GetBalance(callCtx, &pb.GetBalanceRequest{})
	if err != nil {
		return nil, err
	}

	addresses := make([]addressBalanceView, len(response.AddressBalances))
	for i, item := range response.AddressBalances {
		addresses[i] = addressBalanceView{
			Address:        item.Address,
			AvailableSompi: item.Available,
			PendingSompi:   item.Pending,
			AvailableKas:   compactKas(item.Available),
			PendingKas:     compactKas(item.Pending),
		}
	}
	sort.Slice(addresses, func(i, j int) bool {
		return addresses[i].Address < addresses[j].Address
	})

	_ = daemonInfo
	return balanceResponse{
		AvailableSompi: response.Available,
		PendingSompi:   response.Pending,
		AvailableKas:   compactKas(response.Available),
		PendingKas:     compactKas(response.Pending),
		Addresses:      addresses,
	}, nil
}

func (b *backendApp) handleListAddresses(ctx context.Context, _ struct{}) (any, error) {
	_, walletClient, tearDown, err := b.connectCurrentDaemon()
	if err != nil {
		return nil, err
	}
	defer tearDown()

	callCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()

	response, err := walletClient.ShowAddresses(callCtx, &pb.ShowAddressesRequest{})
	if err != nil {
		return nil, err
	}
	return addressesResponse{Addresses: response.Address}, nil
}

func (b *backendApp) handleNewAddress(ctx context.Context, _ struct{}) (any, error) {
	_, walletClient, tearDown, err := b.connectCurrentDaemon()
	if err != nil {
		return nil, err
	}
	defer tearDown()

	callCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()

	response, err := walletClient.NewAddress(callCtx, &pb.NewAddressRequest{})
	if err != nil {
		return nil, err
	}
	return newAddressResponse{Address: response.Address}, nil
}

func (b *backendApp) handleCreateUnsigned(ctx context.Context, request createUnsignedRequest) (any, error) {
	daemonInfo, walletClient, tearDown, err := b.connectCurrentDaemon()
	if err != nil {
		return nil, err
	}
	defer tearDown()

	requestAmount, feePolicy, err := buildSendRequest(request)
	if err != nil {
		return nil, err
	}

	callCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()

	response, err := walletClient.CreateUnsignedTransactions(callCtx, &pb.CreateUnsignedTransactionsRequest{
		From:                     request.FromAddresses,
		Address:                  request.ToAddress,
		Amount:                   requestAmount,
		IsSendAll:                request.SendAll,
		UseExistingChangeAddress: request.UseExistingChangeAddress,
		FeePolicy:                feePolicy,
	})
	if err != nil {
		return nil, err
	}

	keysFile, err := loadWalletKeys(daemonInfo.Network, daemonInfo.KeysFile)
	if err != nil {
		return nil, err
	}

	return bundleResponseFromTransactionsHex(daemonInfo.Network, keysFile, walletserver.EncodeTransactionsToHex(response.UnsignedTransactions))
}

func (b *backendApp) handleSign(_ context.Context, request signRequest) (any, error) {
	request.KeysFile = expandUserPath(request.KeysFile)
	params, err := paramsForNetwork(request.Network)
	if err != nil {
		return nil, err
	}
	if request.Password == "" {
		return nil, fmt.Errorf("password is required")
	}

	keysFile, err := keys.ReadKeysFile(params, request.KeysFile)
	if err != nil {
		return nil, err
	}
	mnemonics, err := keysFile.DecryptMnemonics(request.Password)
	if err != nil {
		return nil, err
	}
	transactions, err := decodeTransactionsHex(request.TransactionsHex)
	if err != nil {
		return nil, err
	}

	updated := make([][]byte, len(transactions))
	for i, transaction := range transactions {
		updated[i], err = libkaspawallet.Sign(params, mnemonics, transaction, keysFile.ECDSA)
		if err != nil {
			return nil, err
		}
	}

	return bundleResponseFromTransactionsHex(request.Network, keysFile, walletserver.EncodeTransactionsToHex(updated))
}

func (b *backendApp) handleBroadcast(ctx context.Context, request broadcastRequest) (any, error) {
	_, walletClient, tearDown, err := b.connectCurrentDaemon()
	if err != nil {
		return nil, err
	}
	defer tearDown()

	transactions, err := decodeTransactionsHex(request.TransactionsHex)
	if err != nil {
		return nil, err
	}

	callCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()

	response, err := walletClient.Broadcast(callCtx, &pb.BroadcastRequest{Transactions: transactions})
	if err != nil {
		return nil, err
	}
	return map[string]any{"txIds": response.TxIDs}, nil
}

func (b *backendApp) handleParse(_ context.Context, request parseRequest) (any, error) {
	var keysFile *keys.File
	var err error
	if request.KeysFile != "" {
		request.KeysFile = expandUserPath(request.KeysFile)
		keysFile, err = loadWalletKeys(request.Network, request.KeysFile)
		if err != nil {
			return nil, err
		}
	}

	return bundleResponseFromTransactionsHex(request.Network, keysFile, request.TransactionsHex)
}

func (b *backendApp) connectCurrentDaemon() (*daemonHandle, pb.KaspawalletdClient, func(), error) {
	b.mu.RLock()
	handle := b.daemon
	b.mu.RUnlock()
	if handle == nil {
		return nil, nil, nil, fmt.Errorf("daemon is not running")
	}
	if handle.FinishedAt != nil {
		message := "daemon is not running"
		if handle.ExitError != "" {
			message = handle.ExitError
		}
		return nil, nil, nil, errors.New(message)
	}

	walletClient, tearDown, err := client.Connect(handle.Address)
	if err != nil {
		return nil, nil, nil, err
	}
	return handle, walletClient, tearDown, nil
}

func (b *backendApp) currentDaemonStatus(ctx context.Context) (daemonStatusResponse, error) {
	b.mu.RLock()
	handle := b.daemon
	b.mu.RUnlock()

	if handle == nil {
		return daemonStatusResponse{State: "stopped", Message: "daemon is not running"}, nil
	}

	response := daemonStatusResponse{
		State:         "starting",
		Message:       "starting local wallet daemon",
		DaemonAddress: handle.Address,
		Network:       handle.Network,
		KeysFile:      handle.KeysFile,
		RPCServer:     handle.RPCServer,
		StartedAt:     handle.StartedAt.Format(time.RFC3339),
	}
	if handle.FinishedAt != nil {
		response.FinishedAt = handle.FinishedAt.Format(time.RFC3339)
	}

	keysFile, err := loadWalletKeys(handle.Network, handle.KeysFile)
	if err == nil {
		wallet := walletSummaryFromKeysFile(handle.Network, keysFile)
		response.Wallet = &wallet
	}

	if handle.ExitError != "" {
		response.State = "stopped"
		response.Message = handle.ExitError
		return response, nil
	}
	if handle.FinishedAt != nil {
		response.State = "stopped"
		response.Message = "daemon exited"
		return response, nil
	}

	walletClient, tearDown, err := client.Connect(handle.Address)
	if err != nil {
		return response, nil
	}
	defer tearDown()

	callCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()

	versionResponse, err := walletClient.GetVersion(callCtx, &pb.GetVersionRequest{})
	if err == nil {
		response.WalletVersion = versionResponse.Version
	}

	balanceResponse, err := walletClient.GetBalance(callCtx, &pb.GetBalanceRequest{})
	if err != nil {
		if strings.Contains(err.Error(), "wallet daemon is not synced yet") {
			response.State = "syncing"
			response.Message = err.Error()
			return response, nil
		}
		response.State = "running"
		response.Message = err.Error()
		return response, nil
	}

	_ = balanceResponse
	response.State = "ready"
	response.Message = "wallet is synced and ready"
	return response, nil
}

func buildSendRequest(request createUnsignedRequest) (uint64, *pb.FeePolicy, error) {
	if (request.SendAll && request.AmountKas != "") || (!request.SendAll && request.AmountKas == "") {
		return 0, nil, fmt.Errorf("exactly one of amountKas or sendAll is required")
	}
	if request.ToAddress == "" {
		return 0, nil, fmt.Errorf("toAddress is required")
	}

	var amount uint64
	var err error
	if !request.SendAll {
		amount, err = utils.KasToSompi(request.AmountKas)
		if err != nil {
			return 0, nil, err
		}
	}

	feeOptions := 0
	if request.FeePolicy.ExactFeeRate != nil {
		feeOptions++
	}
	if request.FeePolicy.MaxFeeRate != nil {
		feeOptions++
	}
	if request.FeePolicy.MaxFee != nil {
		feeOptions++
	}
	if feeOptions > 1 {
		return 0, nil, fmt.Errorf("at most one fee policy may be specified")
	}

	if request.FeePolicy.ExactFeeRate != nil {
		return amount, &pb.FeePolicy{
			FeePolicy: &pb.FeePolicy_ExactFeeRate{ExactFeeRate: *request.FeePolicy.ExactFeeRate},
		}, nil
	}
	if request.FeePolicy.MaxFeeRate != nil {
		return amount, &pb.FeePolicy{
			FeePolicy: &pb.FeePolicy_MaxFeeRate{MaxFeeRate: *request.FeePolicy.MaxFeeRate},
		}, nil
	}
	if request.FeePolicy.MaxFee != nil {
		return amount, &pb.FeePolicy{
			FeePolicy: &pb.FeePolicy_MaxFee{MaxFee: *request.FeePolicy.MaxFee},
		}, nil
	}

	return amount, nil, nil
}

func bundleResponseFromTransactionsHex(network string, keysFile *keys.File, transactionsHex string) (transactionBundleResponse, error) {
	transactions, err := decodeTransactionsHex(transactionsHex)
	if err != nil {
		return transactionBundleResponse{}, err
	}

	params, err := paramsForNetwork(network)
	if err != nil {
		return transactionBundleResponse{}, err
	}

	views := make([]parsedTransactionView, len(transactions))
	fullySigned := true
	for i, transaction := range transactions {
		view, currentFullySigned, err := parseTransaction(params, keysFile, transaction, i)
		if err != nil {
			return transactionBundleResponse{}, err
		}
		views[i] = view
		fullySigned = fullySigned && currentFullySigned
	}

	return transactionBundleResponse{
		TransactionsHex:  transactionsHex,
		TransactionCount: len(transactions),
		FullySigned:      fullySigned,
		Transactions:     views,
	}, nil
}

func parseTransaction(params *dagconfig.Params, keysFile *keys.File, raw []byte, index int) (parsedTransactionView, bool, error) {
	partiallySignedTransaction, err := serialization.DeserializePartiallySignedTransaction(raw)
	if err != nil {
		return parsedTransactionView{}, false, err
	}

	isFullySigned, err := libkaspawallet.IsTransactionFullySigned(raw)
	if err != nil {
		return parsedTransactionView{}, false, err
	}

	view := parsedTransactionView{
		Index:       index + 1,
		TxID:        consensushashing.TransactionID(partiallySignedTransaction.Tx).String(),
		FullySigned: isFullySigned,
		InputCount:  len(partiallySignedTransaction.Tx.Inputs),
		OutputCount: len(partiallySignedTransaction.Tx.Outputs),
	}

	var allInputSompi uint64
	view.Inputs = make([]parsedInputView, len(partiallySignedTransaction.Tx.Inputs))
	view.Signatures = make([]inputSignatureProgressView, len(partiallySignedTransaction.Tx.Inputs))
	for inputIndex, input := range partiallySignedTransaction.Tx.Inputs {
		prevOutput := partiallySignedTransaction.PartiallySignedInputs[inputIndex].PrevOutput
		allInputSompi += prevOutput.Value
		view.Inputs[inputIndex] = parsedInputView{
			Outpoint:    fmt.Sprintf("%s:%d", input.PreviousOutpoint.TransactionID, input.PreviousOutpoint.Index),
			AmountSompi: prevOutput.Value,
			AmountKas:   compactKas(prevOutput.Value),
		}

		signedBy := 0
		for _, pair := range partiallySignedTransaction.PartiallySignedInputs[inputIndex].PubKeySignaturePairs {
			if pair.Signature != nil {
				signedBy++
			}
		}
		minimumSignatures := partiallySignedTransaction.PartiallySignedInputs[inputIndex].MinimumSignatures
		missing := uint32(0)
		if uint32(signedBy) < minimumSignatures {
			missing = minimumSignatures - uint32(signedBy)
		}
		view.Signatures[inputIndex] = inputSignatureProgressView{
			InputIndex:        inputIndex,
			SignedBy:          signedBy,
			MinimumSignatures: minimumSignatures,
			MissingSignatures: missing,
		}
	}

	var allOutputSompi uint64
	view.Outputs = make([]parsedOutputView, len(partiallySignedTransaction.Tx.Outputs))
	for outputIndex, output := range partiallySignedTransaction.Tx.Outputs {
		scriptPublicKeyType, scriptPublicKeyAddress, err := txscript.ExtractScriptPubKeyAddress(output.ScriptPublicKey, params)
		if err != nil {
			return parsedTransactionView{}, false, err
		}

		address := scriptPublicKeyAddress.EncodeAddress()
		if scriptPublicKeyType == txscript.NonStandardTy {
			address = fmt.Sprintf("<non-standard:%s>", hex.EncodeToString(output.ScriptPublicKey.Script))
		}

		view.Outputs[outputIndex] = parsedOutputView{
			Address:     address,
			AmountSompi: output.Value,
			AmountKas:   compactKas(output.Value),
		}
		allOutputSompi += output.Value
	}

	view.FeeSompi = allInputSompi - allOutputSompi
	view.FeeKas = compactKas(view.FeeSompi)

	if keysFile != nil {
		calculator := txmass.NewCalculator(params.MassPerTxByte, params.MassPerScriptPubKeyByte, params.MassPerSigOp)
		mass, err := walletserver.EstimateMassAfterSignatures(partiallySignedTransaction, keysFile.ECDSA, keysFile.MinimumSignatures, calculator)
		if err != nil {
			return parsedTransactionView{}, false, err
		}
		view.Mass = mass
		view.HasMassEstimate = true
		if mass > 0 {
			view.FeeRate = float64(view.FeeSompi) / float64(mass)
		}
	}

	return view, isFullySigned, nil
}

func walletSummaryFromKeysFile(network string, keysFile *keys.File) walletSummaryResponse {
	sortedPublicKeys := append([]string{}, keysFile.ExtendedPublicKeys...)
	sort.Strings(sortedPublicKeys)

	ownedKeyCount := len(keysFile.EncryptedMnemonics)
	publicKeyCount := len(keysFile.ExtendedPublicKeys)
	return walletSummaryResponse{
		Network:                 network,
		KeysFile:                keysFile.Path(),
		PublicKeys:              append([]string{}, keysFile.ExtendedPublicKeys...),
		SortedPublicKeys:        sortedPublicKeys,
		PublicKeyCount:          publicKeyCount,
		OwnedKeyCount:           ownedKeyCount,
		MinimumSignatures:       keysFile.MinimumSignatures,
		CosignerIndex:           keysFile.CosignerIndex,
		LastUsedExternalIndex:   keysFile.LastUsedExternalIndex(),
		LastUsedInternalIndex:   keysFile.LastUsedInternalIndex(),
		Fingerprint:             publicKeyFingerprint(keysFile.ExtendedPublicKeys),
		IsMultisig:              publicKeyCount > 1,
		HasPrivateKeys:          ownedKeyCount > 0,
		OwnsAllKeys:             ownedKeyCount == publicKeyCount,
		IsCanonicalAddressOwner: publicKeyCount > 1 && keysFile.CosignerIndex == 0,
		ECDSA:                   keysFile.ECDSA,
	}
}

func loadWalletKeys(network, keysFilePath string) (*keys.File, error) {
	params, err := paramsForNetwork(network)
	if err != nil {
		return nil, err
	}
	return keys.ReadKeysFile(params, expandUserPath(keysFilePath))
}

func paramsForNetwork(network string) (*dagconfig.Params, error) {
	switch strings.ToLower(strings.TrimSpace(network)) {
	case "", "mainnet":
		return &dagconfig.MainnetParams, nil
	case "testnet":
		return &dagconfig.TestnetParams, nil
	case "devnet":
		return &dagconfig.DevnetParams, nil
	case "simnet":
		return &dagconfig.SimnetParams, nil
	default:
		return nil, fmt.Errorf("unknown network %q", network)
	}
}

func waitForDaemonReady(ctx context.Context, address string, timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		walletClient, tearDown, err := client.Connect(address)
		if err == nil {
			callCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
			_, versionErr := walletClient.GetVersion(callCtx, &pb.GetVersionRequest{})
			cancel()
			tearDown()
			if versionErr == nil {
				return nil
			}
		}

		if time.Now().After(deadline) {
			return fmt.Errorf("timed out waiting for daemon on %s", address)
		}
		time.Sleep(150 * time.Millisecond)
	}
}

func findFreeLoopbackAddress() (string, error) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return "", err
	}
	defer listener.Close()
	return listener.Addr().String(), nil
}

func decodeTransactionsHex(transactionsHex string) ([][]byte, error) {
	if strings.TrimSpace(transactionsHex) == "" {
		return nil, fmt.Errorf("transactionsHex is required")
	}
	return walletserver.DecodeTransactionsFromHex(strings.TrimSpace(transactionsHex))
}

func compactKas(amount uint64) string {
	return strings.TrimSpace(utils.FormatKas(amount))
}

func publicKeyFingerprint(publicKeys []string) string {
	sortedKeys := append([]string{}, publicKeys...)
	sort.Strings(sortedKeys)
	sum := sha256.Sum256([]byte(strings.Join(sortedKeys, "|")))
	return hex.EncodeToString(sum[:8])
}

func hasDuplicateStrings(items []string) bool {
	seen := make(map[string]struct{}, len(items))
	for _, item := range items {
		if _, ok := seen[item]; ok {
			return true
		}
		seen[item] = struct{}{}
	}
	return false
}

func pathExists(path string) (bool, error) {
	_, err := os.Stat(path)
	if err == nil {
		return true, nil
	}
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	return false, err
}

func expandUserPath(path string) string {
	if path == "" || path == "~" {
		return path
	}
	if !strings.HasPrefix(path, "~/") {
		return path
	}

	homeDir, err := os.UserHomeDir()
	if err != nil {
		return path
	}
	return filepath.Join(homeDir, path[2:])
}
