// Generates the signed SAML fixtures for crates/aws-iam/src/xmldsig/testdata with xml-crypto (an independent XML-DSig
// implementation), so the Rust canonicalization is proven against someone else's.
//
// Not run by the tests. To regenerate, copy this file to a scratch directory (xml-crypto is not a dependency of the repo):
//
//   cd <scratch dir> && bun add xml-crypto @xmldom/xmldom
//   openssl genrsa -out key.pem 2048 && openssl req -new -x509 -key key.pem -out cert.pem -days 3650 -subj "/CN=idp.example.com"
//   openssl genrsa -out other-key.pem 2048 && openssl req -new -x509 -key other-key.pem -out other-cert.pem -days 3650 -subj "/CN=other.example.com"
//   bun gen.ts <out dir>
import { SignedXml } from "xml-crypto";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";

const out = process.argv[2] ?? "out";
mkdirSync(out, { recursive: true });
const key = readFileSync("key.pem");
const cert = readFileSync("cert.pem", "utf8");
const otherCert = readFileSync("other-cert.pem", "utf8");

const EXC = "http://www.w3.org/2001/10/xml-exc-c14n#";
const ENV = "http://www.w3.org/2000/09/xmldsig#enveloped-signature";
const ALGS = {
  sha1: ["http://www.w3.org/2000/09/xmldsig#sha1", "http://www.w3.org/2000/09/xmldsig#rsa-sha1"],
  sha256: ["http://www.w3.org/2001/04/xmlenc#sha256", "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"],
  sha512: ["http://www.w3.org/2001/04/xmlenc#sha512", "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512"],
} as const;

/** Signs the element `name` (the first one, by local name) of `xml`, the signature placed after its own Issuer. */
function sign(xml: string, name: "Response" | "Assertion", alg: keyof typeof ALGS, prefixList: string[] = [], signedInfoPrefixList?: string): string {
  const [digest, method] = ALGS[alg];
  const sig = new SignedXml({ privateKey: key, publicCert: cert, inclusiveNamespacesPrefixList: signedInfoPrefixList });
  sig.canonicalizationAlgorithm = EXC;
  sig.signatureAlgorithm = method;
  sig.addReference({
    xpath: `//*[local-name(.)='${name}']`,
    digestAlgorithm: digest,
    transforms: [ENV, EXC],
    inclusiveNamespacesPrefixList: prefixList,
  });
  sig.computeSignature(xml, {
    prefix: "ds",
    location: { reference: `//*[local-name(.)='${name}']/*[local-name(.)='Issuer']`, action: "after" },
  });
  return sig.getSignedXml();
}

const assertion = (id: string) => `<saml:Assertion ID="${id}" Version="2.0" IssueInstant="2026-11-15T00:00:00Z">
    <saml:Issuer>https://idp.example.com/saml</saml:Issuer>
    <!-- comments are not signed -->
    <saml:Subject>
      <saml:NameID Format="urn:oasis:names:tc:SAML:2.0:nameid-format:persistent">alice&amp;co &lt;a&gt;</saml:NameID>
      <saml:SubjectConfirmation Method="urn:oasis:names:tc:SAML:2.0:cm:bearer">
        <saml:SubjectConfirmationData Recipient="https://signin.aws.amazon.com/saml" NotOnOrAfter="2030-01-01T00:00:00Z"/>
      </saml:SubjectConfirmation>
    </saml:Subject>
    <saml:Conditions NotBefore="2026-01-01T00:00:00Z" NotOnOrAfter="2030-01-01T00:00:00Z">
      <saml:AudienceRestriction><saml:Audience>urn:amazon:webservices</saml:Audience></saml:AudienceRestriction>
    </saml:Conditions>
    <AttributeStatement xmlns="urn:oasis:names:tc:SAML:2.0:assertion">
      <Attribute Name="https://aws.amazon.com/SAML/Attributes/Role" FriendlyName="role &quot;r&quot;&#xA;two">
        <AttributeValue xsi:type="xs:string">arn:aws:iam::000000000000:role/r,arn:aws:iam::000000000000:saml-provider/idp</AttributeValue>
      </Attribute>
      <Attribute Name="https://aws.amazon.com/SAML/Attributes/RoleSessionName">
        <AttributeValue><![CDATA[alice@example.com]]></AttributeValue>
      </Attribute>
      <Attribute Name="urn:example:extra">
        <AttributeValue><x:Thing xmlns:x="urn:x" b="2" a="1" x:c="3"><Plain xmlns="">t</Plain></x:Thing></AttributeValue>
      </Attribute>
    </AttributeStatement>
  </saml:Assertion>`;

const response = (inner: string) => `<?xml version="1.0" encoding="UTF-8"?>
<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" ID="_r1" Version="2.0" IssueInstant="2026-11-15T00:00:00Z" Destination="https://signin.aws.amazon.com/saml">
  <saml:Issuer>https://idp.example.com/saml</saml:Issuer>
  <samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Success"/></samlp:Status>
  ${inner}
</samlp:Response>`;

// The Response signed (SHA-256), the assertion inside it unsigned; `xs` named in the PrefixList for the xsi:type value.
writeFileSync(`${out}/response-signed.xml`, sign(response(assertion("_a1")), "Response", "sha256", ["xs"], "xs"));
// The Assertion signed (SHA-512), the Response around it not.
writeFileSync(`${out}/assertion-signed.xml`, sign(response(assertion("_a1")), "Assertion", "sha512", ["xs"]));
// Both signed: the Assertion (SHA-1), then the Response (SHA-256) over it.
writeFileSync(`${out}/both-signed.xml`, sign(sign(response(assertion("_a1")), "Assertion", "sha1"), "Response", "sha256", ["xs", "#default"]));

const b64 = (pem: string) => pem.replace(/-----[^-]+-----/g, "").trim();
const metadata = (signing: string, encryption: string) => `<?xml version="1.0"?>
<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" entityID="https://idp.example.com/saml">
  <md:IDPSSODescriptor protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">
    <md:KeyDescriptor use="signing">
      <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:X509Data><ds:X509Certificate>
${signing}
      </ds:X509Certificate></ds:X509Data></ds:KeyInfo>
    </md:KeyDescriptor>
    <md:KeyDescriptor use="encryption">
      <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:X509Data><ds:X509Certificate>${encryption}</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
    </md:KeyDescriptor>
    <md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect" Location="https://idp.example.com/sso"/>
  </md:IDPSSODescriptor>
</md:EntityDescriptor>
`;
writeFileSync(`${out}/metadata.xml`, metadata(b64(cert), b64(otherCert).replace(/\n/g, "")));
writeFileSync(`${out}/other-metadata.xml`, metadata(b64(otherCert), b64(cert).replace(/\n/g, "")));

// xml-crypto's own check of what it wrote, every signature in each file.
import { DOMParser } from "@xmldom/xmldom";
for (const name of ["response-signed", "assertion-signed", "both-signed"]) {
  const xml = readFileSync(`${out}/${name}.xml`, "utf8");
  const doc = new DOMParser().parseFromString(xml, "text/xml");
  const sigs = Array.from(doc.getElementsByTagNameNS("http://www.w3.org/2000/09/xmldsig#", "Signature"));
  for (const node of sigs) {
    const v = new SignedXml({ publicCert: cert, getCertFromKeyInfo: () => null });
    v.loadSignature(node);
    if (!v.checkSignature(xml)) throw new Error(`${name}: xml-crypto refuses its own signature`);
  }
  console.log(`${name}: ${sigs.length} signature(s) verified by xml-crypto`);
}

// xml-crypto's exclusive canonical form of the assertion in response-signed.xml: what `verify` gives back for it.
import { ExclusiveCanonicalization } from "xml-crypto";
{
  const doc = new DOMParser().parseFromString(readFileSync(`${out}/response-signed.xml`, "utf8"), "text/xml");
  const a = doc.getElementsByTagNameNS("urn:oasis:names:tc:SAML:2.0:assertion", "Assertion")[0];
  writeFileSync(`${out}/assertion-c14n.xml`, new ExclusiveCanonicalization().process(a as any, {}) as string);
}
