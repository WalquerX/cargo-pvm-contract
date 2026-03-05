// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface ReturnValues {
    function getPair() external view returns (uint256, bool);
    function getTriple() external view returns (uint256, address, bool);
    function identity(uint256 val) external view returns (uint256);
}
