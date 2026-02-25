// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface CompositeTypes {
    function sumFixedArray(uint256[3] scores) external view returns (uint256);
    function getFixedArray() external view returns (uint256[3] memory);
    function processTuple((uint256,bool) data) external view returns (uint256);
}
