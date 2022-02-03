import {Stack, StackProps} from 'aws-cdk-lib';
import {Construct} from 'constructs';
import * as apigateway from "aws-cdk-lib/aws-apigateway";
import * as lambda from "aws-cdk-lib/aws-lambda";
import * as iam from 'aws-cdk-lib/aws-iam';
import * as s3 from "aws-cdk-lib/aws-s3";

// import * as sqs from 'aws-cdk-lib/aws-sqs';

export class SigningServiceInfraStack extends Stack {
  constructor(scope: Construct, id: string, props?: StackProps) {
    super(scope, id, props);

    const bucket = new s3.Bucket(this, "signing_bot");

    const handler = new lambda.Function(this, "SignerBackend", {
      runtime: lambda.Runtime.PROVIDED_AL2,
      code: lambda.Code.fromAsset('resources/lambda/'),
      handler: 'not.required',
      environment: {
        BUCKET: bucket.bucketName
      }
    });

    bucket.grantReadWrite(handler);

    const lambdaKmsPolicyStmt = new iam.PolicyStatement({
      actions: ['kms:GenerateRandom'],
      resources: ['*']
    });
    handler.role?.attachInlinePolicy(
        new iam.Policy(this, "lambda-generate-random-policy", {
          statements: [lambdaKmsPolicyStmt]
        })
    );

    const api = new apigateway.LambdaRestApi(this, 'SignerApi', {
      handler: handler,
      proxy: false
    });

    const pubkeys = api.root.addResource('keys');
    pubkeys.addMethod("POST"); // POST keyname to /keys to create a new key

    const pubkey = pubkeys.addResource("{key}");
    pubkey.addMethod("GET"); // GET /keys/{key} to get an xpub
    pubkey.addMethod("POST"); // POST bluewallet export to /keys/{key} to create a new wallet

    const wallet = pubkey.addResource("wallet");
    wallet.addMethod("POST"); // POST psbt to /keys/{key}/wallet to sign

  }
}
