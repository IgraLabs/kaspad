package main

import (
	"fmt"
	"os"

	"github.com/kaspanet/kaspad/cmd/kaspawallet/daemon/server"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/keys"
	"github.com/kaspanet/kaspad/cmd/kaspawallet/libkaspawallet"
)

func signExitProposal(conf *signExitProposalConfig) error {
	material, err := verifyExitProposalForSigning(conf)
	if err != nil {
		return err
	}
	fmt.Fprintf(os.Stderr, "Verified proposal %s. Decrypting local wallet for signing.\n", material.Result.ProposalHash)

	keysFile, err := keys.ReadKeysFile(conf.NetParams(), conf.KeysFile)
	if err != nil {
		return err
	}

	if len(conf.Password) == 0 {
		conf.Password = keys.GetPassword("Password:")
	}
	privateKeys, err := keysFile.DecryptMnemonics(conf.Password)
	if err != nil {
		return err
	}

	updatedPartiallySignedTransactions := make([][]byte, len(material.UnsignedBundleParts))
	for i, partiallySignedTransaction := range material.UnsignedBundleParts {
		updatedPartiallySignedTransactions[i], err =
			libkaspawallet.Sign(conf.NetParams(), privateKeys, partiallySignedTransaction, keysFile.ECDSA)
		if err != nil {
			return err
		}
	}

	areAllTransactionsFullySigned := true
	for _, updatedPartiallySignedTransaction := range updatedPartiallySignedTransactions {
		isFullySigned, err := libkaspawallet.IsTransactionFullySigned(updatedPartiallySignedTransaction)
		if err != nil {
			return err
		}
		if !isFullySigned {
			areAllTransactionsFullySigned = false
		}
	}

	if areAllTransactionsFullySigned {
		fmt.Fprintln(os.Stderr, "The proposal is signed and ready to broadcast")
	} else {
		fmt.Fprintln(os.Stderr, "Successfully signed proposal")
	}

	fmt.Println(server.EncodeTransactionsToHex(updatedPartiallySignedTransactions))
	return nil
}
