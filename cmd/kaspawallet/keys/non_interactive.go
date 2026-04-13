package keys

import "github.com/kaspanet/kaspad/domain/dagconfig"

// EncryptMnemonicsAndPublicKeys builds the encrypted mnemonic payloads and
// matching extended public keys without prompting for terminal input.
func EncryptMnemonicsAndPublicKeys(params *dagconfig.Params, mnemonics []string, password string, isMultisig bool) (
	encryptedPrivateKeys []*EncryptedMnemonic, extendedPublicKeys []string, err error) {

	return encryptedMnemonicExtendedPublicKeyPairs(params, mnemonics, password, isMultisig)
}
