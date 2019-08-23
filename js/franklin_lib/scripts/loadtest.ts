import BN = require('bn.js');
import { Wallet } from '../src/wallet';
import { ethers } from 'ethers';
import {bigNumberify, parseEther} from "ethers/utils";

function sleep(ms) {
    return new Promise(resolve => {
        setTimeout(resolve, ms);
    });
}

async function main() {
    const provider = new ethers.providers.JsonRpcProvider(process.env.WEB3_URL);
    let ethWallet = ethers.Wallet.fromMnemonic(process.env.MNEMONIC, "m/44'/60'/0'/0/1").connect(provider);
    let wallet = await Wallet.fromEthWallet(ethWallet);
    let ethWallet2 = ethers.Wallet.fromMnemonic(process.env.MNEMONIC, "m/44'/60'/0'/0/2").connect(provider);
    let wallet2 = await Wallet.fromEthWallet(ethWallet2);

    await wallet.updateState();
    await wallet2.updateState();


    // console.log(await wallet.depositOffchain(wallet.supportedTokens['0'], new BN(18), new BN(2)));
    // await wallet.updateState();
    // console.log(wallet.franklinState);

    console.log(await wallet.depositOnchain(wallet.supportedTokens['0'], 20));
    await sleep(5000);
    console.log(await wallet.depositOffchain(wallet.supportedTokens['0'], 18, 2));
    await wallet.waitPendingTxsExecuted();
    console.log(await wallet.transfer(wallet2.address, wallet.supportedTokens['0'], 15,3));
    await wallet.waitPendingTxsExecuted();
    console.log(await wallet2.widthdrawOffchain(wallet2.supportedTokens['0'], 10, 5));
    // console.log(await wallet2.widthdrawOnchain(wallet2.supportedTokens['0'],bigNumberify(1));
    await wallet.waitPendingTxsExecuted();

    await wallet2.updateState();
    console.log("offchain 2", wallet2.franklinState);
    console.log("onchain 2", wallet2.ethState);
}

main();