package main

import (
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"strings"

	"github.com/kaspanet/kaspad/cmd/kaspawallet/keys"
	"github.com/pkg/errors"
)

type federationListResponse struct {
	Results []federationAPI `json:"results"`
	Next    string          `json:"next"`
}

type federationAPI struct {
	ID              string   `json:"id"`
	Network         string   `json:"network"`
	Threshold       uint32   `json:"threshold"`
	ECDSA           bool     `json:"ecdsa"`
	Xpubs           []string `json:"xpubs"`
	XpubFingerprint string   `json:"xpub_fingerprint"`
}

func (federation *federationAPI) UnmarshalJSON(data []byte) error {
	type alias federationAPI
	aux := struct {
		*alias
		XpubFingerprintCamel string `json:"xpubFingerprint"`
	}{alias: (*alias)(federation)}
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	if federation.XpubFingerprint == "" {
		federation.XpubFingerprint = aux.XpubFingerprintCamel
	}
	return nil
}

type proposalListResponse struct {
	Results []exitProposalAPI `json:"results"`
	Next    string            `json:"next"`
}

type exitProposalCandidateListResult struct {
	ProposalHash string   `json:"proposal_hash"`
	Status       string   `json:"status"`
	OK           bool     `json:"ok"`
	KaspaTxID    string   `json:"kaspa_tx_id,omitempty"`
	EvidenceHash string   `json:"evidence_hash,omitempty"`
	Error        string   `json:"error,omitempty"`
	Checks       []string `json:"checks,omitempty"`
}

func listExitProposals(conf *listExitProposalsConfig) error {
	params := conf.NetParams()
	if params == nil {
		return errors.New("network parameters are required")
	}
	keysFile, err := keys.ReadKeysFile(params, conf.KeysFile)
	if err != nil {
		return err
	}

	baseURL := strings.TrimRight(conf.SafeURL, "/")
	federation, err := findSafeServiceFederation(baseURL, keysFile, kaspaEvidenceNetwork(params))
	if err != nil {
		return err
	}

	proposals, err := fetchFederationProposals(baseURL, federation.ID)
	if err != nil {
		return err
	}

	results := make([]exitProposalCandidateListResult, 0, len(proposals))
	for _, proposal := range proposals {
		if proposal.ProposalHash == "" {
			continue
		}
		result := exitProposalCandidateListResult{
			ProposalHash: proposal.ProposalHash,
			Status:       proposal.Status,
		}
		material, err := verifyExitProposalWithOptions(exitProposalVerifyOptions{
			KeysFile:     conf.KeysFile,
			SafeURL:      conf.SafeURL,
			ProposalHash: proposal.ProposalHash,
			IgraRPCURL:   conf.IgraRPCURL,
			KaspaRPCURL:  conf.KaspaRPCURL,
			NetParams:    params,
		})
		if err != nil {
			result.Error = err.Error()
			if conf.IncludeInvalid {
				results = append(results, result)
			}
			continue
		}
		result.OK = true
		result.KaspaTxID = material.Result.KaspaTxID
		result.EvidenceHash = material.Result.EvidenceHash
		result.Checks = material.Result.Checks
		results = append(results, result)
	}

	if conf.JSON {
		encoded, err := json.MarshalIndent(results, "", "  ")
		if err != nil {
			return err
		}
		fmt.Println(string(encoded))
		return nil
	}

	fmt.Fprintf(os.Stderr, "Federation %s: %d candidates fetched, %d displayed\n", federation.ID, len(proposals), len(results))
	for _, result := range results {
		if result.OK {
			fmt.Printf("OK %s status=%s tx=%s evidence=%s\n", result.ProposalHash, result.Status, result.KaspaTxID, result.EvidenceHash)
			continue
		}
		fmt.Printf("INVALID %s status=%s error=%s\n", result.ProposalHash, result.Status, result.Error)
	}
	return nil
}

func findSafeServiceFederation(baseURL string, keysFile *keys.File, network string) (federationAPI, error) {
	xpubs := sortedStrings(keysFile.ExtendedPublicKeys)
	fingerprint, err := canonicalJSONHashValue(xpubs)
	if err != nil {
		return federationAPI{}, err
	}

	federations, err := fetchFederations(baseURL)
	if err != nil {
		return federationAPI{}, err
	}
	for _, federation := range federations {
		if federation.Network != network {
			continue
		}
		if federation.Threshold != keysFile.MinimumSignatures {
			continue
		}
		if federation.ECDSA != keysFile.ECDSA {
			continue
		}
		if federation.XpubFingerprint != "" && federation.XpubFingerprint == fingerprint {
			return federation, nil
		}
		keysMatch, err := sameExtendedPublicKeys(federation.Xpubs, keysFile.ExtendedPublicKeys)
		if err == nil && keysMatch {
			return federation, nil
		}
	}
	return federationAPI{}, errors.Errorf("no safe-service federation matches local wallet public keys on %s", network)
}

func fetchFederations(baseURL string) ([]federationAPI, error) {
	firstURL := fmt.Sprintf("%s/api/v1/kaspa/federations/", baseURL)
	var out []federationAPI
	for nextURL := firstURL; nextURL != ""; {
		body, err := httpGetBytes(nextURL)
		if err != nil {
			return nil, err
		}
		var page federationListResponse
		if err := json.Unmarshal(body, &page); err == nil && page.Results != nil {
			out = append(out, page.Results...)
			nextURL = normalizeSafeServiceNextURL(baseURL, page.Next)
			continue
		}
		var raw []federationAPI
		if err := json.Unmarshal(body, &raw); err != nil {
			return nil, errors.Wrap(err, "failed to decode safe-service federation list")
		}
		out = append(out, raw...)
		break
	}
	return out, nil
}

func fetchFederationProposals(baseURL string, federationID string) ([]exitProposalAPI, error) {
	firstURL := fmt.Sprintf("%s/api/v1/kaspa/federations/%s/transactions/", baseURL, url.PathEscape(federationID))
	var out []exitProposalAPI
	for nextURL := firstURL; nextURL != ""; {
		body, err := httpGetBytes(nextURL)
		if err != nil {
			return nil, err
		}
		var page proposalListResponse
		if err := json.Unmarshal(body, &page); err == nil && page.Results != nil {
			out = append(out, page.Results...)
			nextURL = normalizeSafeServiceNextURL(baseURL, page.Next)
			continue
		}
		var raw []exitProposalAPI
		if err := json.Unmarshal(body, &raw); err != nil {
			return nil, errors.Wrap(err, "failed to decode safe-service proposal list")
		}
		out = append(out, raw...)
		break
	}
	return out, nil
}

func normalizeSafeServiceNextURL(baseURL string, nextURL string) string {
	nextURL = strings.TrimSpace(nextURL)
	if nextURL == "" {
		return ""
	}
	if strings.HasPrefix(nextURL, "http://") || strings.HasPrefix(nextURL, "https://") {
		return nextURL
	}
	return strings.TrimRight(baseURL, "/") + "/" + strings.TrimLeft(nextURL, "/")
}
