# SigningService (maybe we'll have a more clever name one day!)

## What is this?

This repo has a little "serverless" (runs on lambda and some other services) multisig signing oracle that will sign (or
not) based on _policy_. The idea is that you can have a 2-of-3 multisig wallet in Bluewallet (or another wallet of your
choice, right now we have a parser for bluewallet multisig setup files, so anything that outputs those files -- Sparrow,
some tool you write, you writing it yourself, etc. should work)
where one key is on the phone, one key is kept safely offline (in case you blow away SigningService or whatever), and
then one key is held by SigningService. When you want to spend, your phone makes one signature, and then you send the
PSBT to SigningService, where it will evaluate the request against a configured set of policies, and if it passes, will
sign the PSBT and return it. From there you load the signed PSBT into Bluewallet, hit "finalize" and are ready to
broadcast the transaction! If the transaction violates any of the policies, then the SigningService refuses to sign it.
At that point the money can't be spent without going and digging out the third "recovery" key.

The goal is to (eventually, after the code gets better and the integration isn't a giant kludge) have the convenience
and portability of a phone wallet but have it be secure enough that you can keep access to reasonably large amount of
money without losing sleep over the risk of malware or theft/loss of your phone
("reasonably large amount of money" to me means something like your checking account -- the funds you expect to spend
over a few weeks or a month, and sits between what you'd normally keep as cash in your wallet, and your "savings" which
should be in some offline multisig setup).

## What policies exist so far?

1. ValuePolicy - set a limit on the maximum amount (excluding change) that can be spent in one transaction
2. AndonPolicy - an "andon cord": if you trigger this policy (by setting a boolean to `true`) then all signings are
   rejected. This is a "big red button" to halt all spends in the case of theft/loss/attack.

3. Right now, these policies have hardcoded values. The plan is to have their configuration stored in DDB, and then have
   a UI (or something) to dial in the desired configuration.

## How to build/deploy

(more detailed instructions coming one day. If any of these things sound confusing, then SigningBot is too early for
you)

1. install npm, cdk, the rust toolchain
2. (if you are on an apple silicon mac) read the directions in `signing_bot/README.md` on how to get the
   cross-compilation setup
3. have AWS creds configured
4. run `make deploy`

## How to use SigningService with Bluewallet

TODO: write me!

## Risks/Things to be aware of

1. This code is SUPER alpha. There are known issues (like key-create not being idempotent!!!) that need to get solved,
   and probably a shitload of bugs. The fact that the integration with Bluewallet is through an iOS shortcut should give
   you the correct amount of discomfort trusting it with real money (for your own definition of "real"). It's been
   tested quite a bit, but caveat emptor, buyer beware, NO WARRENTY, etc.
2. Right now SigningService stores its keys in an S3 bucket. That's it, that's all it does to protect them is a
   non-public S3 bucket.
3. TODO: write more things that are horrible about the current state of this codebase

## API

From the endpoint you get out of deploying the stack:

    POST `key_name=[key name]` to /keys -> create new key
    GET /keys/{key} -> get xpub # not implemented yet
    POST bluewallet_export to /keys/{key} -> create wallet
    POST psbt to /keys/{key}/wallet -> sign psbt

## Future plans

- More (and more interesting) policies
    - max spend in X hours/days/weeks
    - auto-deny list, auto-approve list (blacklist and whitelist)
- Encrypt private keys (either credstash-style with KMS wrapping, or possibly just SSE in s3, unclear)
- MFA (or some other mechanism) to override denials
- Some easy-to-get-too mechanism to trigger the Andon Cord policy (send a text to a phone number, button on a website,
  iot button, etc)
- Some UI (authenticated website?) to set policy parameters (spend threshold, blocked hours, address whitelist, etc)
- multi-step spend paths with network monitors/watchtowers to enforce policy (until we get convenents -- basically what
  Revault does)
- proper (not the Shortcuts HACK) integration into Bluewallet or another wallet
