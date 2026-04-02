module('Branch', {
  
  setup: function() {
    this.node1 = new Tree.Node();
    this.node2 = new Tree.Node();
    this.node3 = new Tree.Node();
    this.branch = new Tree.Branch([this.node1, this.node2, this.node3]);
  },
  
  teardown: function() {
    delete this.node1;
    delete this.node2;
    delete this.node3;
    delete this.branch;
  }
  
});

test('size', function() {
  equals(this.branch.size(), 3);
});

test('append node', function() {
  var node = new Tree.Node({}, []);
  this.branch.append(node);
  
  equals(this.branch.size(), 4);
  equals(_.last(this.branch.nodes), node);
  equals(node.branch, this.branch);
});

test('insert node', function() {
  var node = new Tree.Node({}, []);
  this.branch.insert(2, node);
  
  equals(this.branch.size(), 4);
  equals(this.branch.nodes[2], node);
  equals(node.branch, this.branch);
  
  node = new Tree.Node({}, []);
  this.branch.insert(9999, node);
  
  equals(_.last(this.branch.nodes), node);
  
  node = new Tree.Node({}, []);
  this.branch.insert(-1, node);
  
  equals(_.first(this.branch.nodes), node);
});

test('remove node', function() {
  var node = this.branch.remove(this.node1);
  
  equals(this.branch.size(), 2);
  equals(_.first(this.branch.nodes), this.node2);
  equals(_.last(this.branch.nodes), this.node3);
  equals(node, this.node1);
  equals(node.branch, null);
});

test('remove node at index', function() {
  this.branch.removeAtIndex(2);
  
  equals(this.branch.size(), 2);
  equals(_.first(this.branch.nodes), this.node1);
  equals(_.last(this.branch.nodes), this.node2);
});