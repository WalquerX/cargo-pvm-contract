// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface Events {
    event ValueChanged(address indexed who, uint256 oldValue, uint256 newValue);
    function setValue(uint256 val) external;
    function getValue() external view returns (uint256);
}
