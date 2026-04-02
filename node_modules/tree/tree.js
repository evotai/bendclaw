//     Tree.js
//     Scott Nelson <scottbnel@gmail.com>
//     https://github.com/scttnlsn/tree.js

(function() {
  
  // Setup
  // --------------------
  
  // Namespacing
  var Tree;
  if (typeof exports !== 'undefined') {
    Tree = exports;
  } else {
    Tree = this.Tree = {};
  }
  
  // Current library version
  Tree.VERSION = '0.1.2';
  
  // Require Underscore if not already present
  var _ = this._;
  if (!_ && (typeof require !== 'undefined')) {
    _ = require('underscore');
  }
  
  // Tree.Node
  // --------------------
  
  // A node is the fundamental unit in a tree.  Each node
  // maintains a branch of children and an optional set of
  // data.  A node that has been inserted into a tree will
  // also belong to a particular branch.  Each node must have
  // a unique ID with with it can be retrieved from a tree.
  
  Tree.Node = function(children, data) {
    this.children = children || new Tree.Branch();
    this.data = data || {};
    
    this.branch = null;
    this.id = this.data.id || _.uniqueId('n');
  };
  
  _.extend(Tree.Node.prototype, {
    
    // Set the index of this node in the parent branch.
    setIndex: function(i) {
      if (this.branch == null) {
        throw new TypeError('Node does not belong to a branch');
      }
      
      var branch = this.branch;
      branch.remove(this);
      branch.insert(i, this);
    }
    
  });
  
  // Tree.Branch
  // --------------------
  
  // A branch is a simple wrapper around an ordered list of
  // nodes.
  
  Tree.Branch = function(nodes) {
    nodes = nodes || [];
    this.nodes = [];
    
    var self = this;
    _.each(nodes, function(node) {
      self.append(node);
    });
  };
  
  _.extend(Tree.Branch.prototype, {
    
    // Append the given node to the end of the branch.
    append: function(node) {
      node.branch = this;
      this.nodes.push(node);
    },
    
    // Insert the given node at specified index in the branch.
    insert: function(i, node) {
      node.branch = this;
      this.nodes.splice(Math.max(0, i), 0, node);
    },
    
    // Remove the given node from the branch.
    remove: function(node) {
      this.nodes = _.reject(this.nodes, function(n) {
        return n.id == node.id;
      });
      
      node.branch = null;
      
      return node;
    },
    
    // Remove the node at the given index from the branch.
    removeAtIndex: function(i) {
      this.nodes.splice(i, 1);
    },
    
    // Return the number of nodes in the branch.
    size: function() {
      return this.nodes.length;
    }
    
  });
  
  // Tree.Root
  // --------------------
  
  // A root is composed of a single branch and a set of methods
  // for recursively operating on the entire tree hierarchy.
  
  Tree.Root = function(branch) {
    this.branch = branch || new Tree.Branch();
  };
  
  _.extend(Tree.Root.prototype, {
    
    // Find the node with the given ID (BFS).
    find: function(id) {
      var queue = [];
      
      _.each(this.branch.nodes, function(node) {
        queue.push(node);
      });
      
      while (queue.length > 0) {
        var node = queue.shift();
        
        if (id == node.id) {
          return node;
        } else {
          _.each(node.children.nodes, function(node) {
            queue.push(node);
          });
        }
      }
      
      return null;
    },
    
    // Insert the given node into the tree under the specified
    // parent ID (node is appended to the end of the branch unless
    // an index is provided).
    insert: function(node, parent_id, i) {
      var parent = this.find(parent_id);
      
      if (i == undefined) {
        parent.children.append(node);
      } else {
        parent.children.insert(i, node);
      }
    },
    
    // Remove the node with the given ID from the tree, returning
    // the node and it's subtree.
    remove: function(id) {
      var node = this.find(id);
      return node.branch.remove(node);
    },
    
    // Set the branch index for the node with the given ID.
    setIndex: function(id, i) {
      var node = this.find(id);
      node.setIndex(i);
    }
    
  });
  
})();